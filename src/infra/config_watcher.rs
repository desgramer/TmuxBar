use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::infra::config::AppConfig;

// Minimum time between two consecutive config reload events.
const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);

/// Watches a config file for modifications and calls a callback with the
/// newly-parsed [`AppConfig`] on each (debounced) change event.
///
/// Drop the `ConfigWatcher` to stop watching.
pub struct ConfigWatcher {
    /// The underlying watcher must be kept alive; dropping it stops the watch.
    _watcher: RecommendedWatcher,
}

impl ConfigWatcher {
    /// Start watching `config_path` for changes.
    ///
    /// When the file is modified:
    /// 1. Events within 500 ms of the last processed event are ignored.
    /// 2. The file is re-parsed via [`AppConfig`] deserialization.
    /// 3. `callback` is called with the new config.
    ///
    /// The callback runs on the notify background thread, so it must be
    /// `Send + 'static`.  Use channels or `Arc<Mutex<>>` to communicate
    /// results back to other threads.
    pub fn start(
        config_path: PathBuf,
        callback: impl Fn(AppConfig) + Send + 'static,
    ) -> Result<Self> {
        // Shared last-event timestamp for debouncing.
        let last_event: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

        let path_for_cb = config_path.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Config watcher error: {e:#}");
                    return;
                }
            };

            // Only react to data-write events (modify / create).
            let is_modify = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_)
            );
            if !is_modify {
                return;
            }

            // Debounce: ignore events within DEBOUNCE_DURATION of the last one.
            {
                let mut last = last_event.lock().unwrap();
                if let Some(prev) = *last {
                    if prev.elapsed() < DEBOUNCE_DURATION {
                        return;
                    }
                }
                *last = Some(Instant::now());
            }

            // Re-parse the config.
            match reload_config(&path_for_cb) {
                Ok(cfg) => {
                    tracing::info!("Config reloaded from {}", path_for_cb.display());
                    callback(cfg);
                }
                Err(e) => {
                    tracing::warn!("Config reload failed: {e:#}");
                }
            }
        })?;

        // Watch the parent directory so we also catch atomic-rename saves
        // (many editors write to a temp file then rename into place, which
        // means the original inode may never see a Modify event).
        let watch_path = config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| config_path.clone());

        watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

        Ok(Self { _watcher: watcher })
    }
}

// ---------------------------------------------------------------------------
// Helper: parse config directly from a path (no home-dir side-effects).
// ---------------------------------------------------------------------------

/// Read and parse an [`AppConfig`] from `path`.
///
/// Exposed as `pub(crate)` so that unit tests can call it without going
/// through the full `AppConfig::load()` (which hard-codes
/// `~/.config/tmuxbar/config.toml`).
pub(crate) fn reload_config(path: &PathBuf) -> Result<AppConfig> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let cfg: AppConfig = toml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Debounce helper (testable in isolation)
// ---------------------------------------------------------------------------

/// Returns `true` if the event should be processed (i.e. not debounced away).
///
/// `last` is updated in-place when the function returns `true`.
pub(crate) fn should_process(last: &mut Option<Instant>, now: Instant) -> bool {
    match *last {
        Some(prev) if now.duration_since(prev) < DEBOUNCE_DURATION => false,
        _ => {
            *last = Some(now);
            true
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
    use std::time::Duration;
    use tempfile::NamedTempFile;

    // ---- debounce logic -------------------------------------------------------

    #[test]
    fn test_first_event_always_processed() {
        let mut last: Option<Instant> = None;
        let now = Instant::now();
        assert!(should_process(&mut last, now));
    }

    #[test]
    fn test_event_within_debounce_ignored() {
        let mut last: Option<Instant> = None;
        let t0 = Instant::now();
        assert!(should_process(&mut last, t0));

        // Advance by less than the debounce window.
        let t1 = t0 + Duration::from_millis(100);
        assert!(!should_process(&mut last, t1));
    }

    #[test]
    fn test_event_after_debounce_processed() {
        let mut last: Option<Instant> = None;
        let t0 = Instant::now();
        assert!(should_process(&mut last, t0));

        // Advance beyond the debounce window.
        let t1 = t0 + DEBOUNCE_DURATION + Duration::from_millis(1);
        assert!(should_process(&mut last, t1));
    }

    #[test]
    fn test_multiple_rapid_events_only_first_passes() {
        let mut last: Option<Instant> = None;
        let t0 = Instant::now();
        assert!(should_process(&mut last, t0));

        for i in 1u64..=10 {
            let t = t0 + Duration::from_millis(i * 10); // 10-100 ms — all < 500 ms
            assert!(!should_process(&mut last, t), "event at {i}*10ms should be debounced");
        }
    }

    #[test]
    fn test_debounce_resets_after_window() {
        let mut last: Option<Instant> = None;
        let t0 = Instant::now();
        should_process(&mut last, t0);

        let t1 = t0 + DEBOUNCE_DURATION + Duration::from_millis(1);
        assert!(should_process(&mut last, t1));

        // Now last == t1; another quick event should be ignored.
        let t2 = t1 + Duration::from_millis(50);
        assert!(!should_process(&mut last, t2));
    }

    // ---- FdAlertPolicy::update_config -----------------------------------------

    #[test]
    fn test_fd_alert_policy_update_config_resets_state() {
        use crate::core::fd_alert_policy::FdAlertPolicy;
        use crate::models::{AlertConfig, AlertLevel};

        let initial = AlertConfig { warn_pct: 85, elevated_pct: 90, crit_pct: 95 };
        let mut policy = FdAlertPolicy::new(initial);

        // Trigger a notification so last_notified_pct is set.
        assert_eq!(policy.evaluate(87), Some(AlertLevel::Warning));

        // The same value should now be suppressed.
        assert_eq!(policy.evaluate(87), None);

        // Update config: this should reset state so the next evaluate fires again.
        let new_config = AlertConfig { warn_pct: 80, elevated_pct: 88, crit_pct: 93 };
        policy.update_config(new_config);

        // After reset, 80 (new warn threshold) should fire Warning again.
        assert_eq!(policy.evaluate(80), Some(AlertLevel::Warning));
    }

    #[test]
    fn test_fd_alert_policy_update_config_applies_new_thresholds() {
        use crate::core::fd_alert_policy::FdAlertPolicy;
        use crate::models::{AlertConfig, AlertLevel};

        let initial = AlertConfig { warn_pct: 85, elevated_pct: 90, crit_pct: 95 };
        let mut policy = FdAlertPolicy::new(initial);

        // 80% is below the initial warn threshold → no alert.
        assert_eq!(policy.evaluate(80), None);

        // Lower the warn threshold to 75%.
        let new_config = AlertConfig { warn_pct: 75, elevated_pct: 85, crit_pct: 92 };
        policy.update_config(new_config);

        // 80% is now above the new warn threshold → Warning.
        assert_eq!(policy.evaluate(80), Some(AlertLevel::Warning));
    }

    // ---- reload_config --------------------------------------------------------

    #[test]
    fn test_reload_config_reads_valid_toml() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        write!(
            tmp,
            r#"
[monitor]
poll_interval_secs = 10
fd_warn_pct = 70
"#
        )
        .unwrap();

        let cfg = reload_config(&tmp.path().to_path_buf()).expect("reload");
        assert_eq!(cfg.monitor.poll_interval_secs, 10);
        assert_eq!(cfg.monitor.fd_warn_pct, 70);
        // Unspecified fields use defaults.
        assert_eq!(cfg.monitor.fd_crit_pct, 95);
    }

    #[test]
    fn test_reload_config_errors_on_invalid_toml() {
        let mut tmp = NamedTempFile::new().expect("tempfile");
        write!(tmp, "not valid toml ][[[").unwrap();
        assert!(reload_config(&tmp.path().to_path_buf()).is_err());
    }

    #[test]
    fn test_reload_config_errors_on_missing_file() {
        let path = PathBuf::from("/tmp/this_file_does_not_exist_tmuxbar_test.toml");
        assert!(reload_config(&path).is_err());
    }

    // ---- integration: watcher fires callback ----------------------------------

    #[test]
    fn test_watcher_fires_callback_on_file_write() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let tmp_dir = tempfile::tempdir().expect("tempdir");
        let config_path = tmp_dir.path().join("config.toml");

        // Write initial config.
        std::fs::write(
            &config_path,
            "[monitor]\npoll_interval_secs = 3\n",
        )
        .expect("write initial config");

        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = Arc::clone(&fired);
        let received_interval = Arc::new(Mutex::new(0u64));
        let received_interval_clone = Arc::clone(&received_interval);

        let _watcher = ConfigWatcher::start(config_path.clone(), move |cfg| {
            *received_interval_clone.lock().unwrap() = cfg.monitor.poll_interval_secs;
            fired_clone.store(true, Ordering::SeqCst);
        })
        .expect("start watcher");

        // Give the watcher a moment to initialise.
        std::thread::sleep(Duration::from_millis(200));

        // Write new config.
        std::fs::write(
            &config_path,
            "[monitor]\npoll_interval_secs = 42\n",
        )
        .expect("write updated config");

        // Wait up to 3 s for the callback to fire.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !fired.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            fired.load(Ordering::SeqCst),
            "ConfigWatcher callback did not fire within timeout"
        );
        assert_eq!(*received_interval.lock().unwrap(), 42);
    }
}
