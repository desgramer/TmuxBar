use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::broadcast;
use tokio::time;
use tracing;

use crate::models::{
    MonitorEvent, SessionStats, SessionStatus, SystemProbe, TmuxAdapter,
};

// ---------------------------------------------------------------------------
// MonitorService
// ---------------------------------------------------------------------------

/// Polls tmux sessions and OS metrics on a fixed interval, broadcasting
/// [`MonitorEvent`]s to all subscribers via a `tokio::sync::broadcast` channel.
pub struct MonitorService {
    tmux: Arc<dyn TmuxAdapter>,
    sys_probe: Arc<dyn SystemProbe>,
    poll_interval: Duration,
    event_tx: broadcast::Sender<MonitorEvent>,
}

impl MonitorService {
    /// Create a new `MonitorService` and return it together with an initial
    /// broadcast receiver.
    ///
    /// * `poll_interval_secs` — polling period in seconds
    /// * `channel_capacity` — bounded broadcast channel capacity
    pub fn new(
        tmux: Arc<dyn TmuxAdapter>,
        sys_probe: Arc<dyn SystemProbe>,
        poll_interval_secs: u64,
        channel_capacity: usize,
    ) -> (Self, broadcast::Receiver<MonitorEvent>) {
        let (event_tx, event_rx) = broadcast::channel(channel_capacity);
        let service = Self {
            tmux,
            sys_probe,
            poll_interval: Duration::from_secs(poll_interval_secs),
            event_tx,
        };
        (service, event_rx)
    }

    /// Obtain a new broadcast receiver. Consumers call this to subscribe to
    /// monitoring events independently of one another.
    pub fn subscribe(&self) -> broadcast::Receiver<MonitorEvent> {
        self.event_tx.subscribe()
    }

    /// Run the monitoring loop. This method never returns under normal
    /// operation — it ticks at the configured `poll_interval` forever.
    ///
    /// Individual tick failures (e.g. tmux not running, sysctl error) are
    /// logged and skipped; the loop keeps going.
    pub async fn run(&self) -> Result<()> {
        let mut interval = time::interval(self.poll_interval);

        loop {
            interval.tick().await;

            match self.collect_event().await {
                Ok(event) => {
                    // Ignore SendError — it just means no receivers are active.
                    let _ = self.event_tx.send(event);
                }
                Err(e) => {
                    tracing::warn!("MonitorService tick failed: {e:#}");
                }
            }
        }
    }

    /// Update the polling interval. Takes effect only for subsequent `run()`
    /// calls, since a running `tokio::time::Interval` cannot be re-configured.
    pub fn update_interval(&mut self, secs: u64) {
        self.poll_interval = Duration::from_secs(secs);
    }

    /// Collect per-session stats by walking windows → panes and aggregating
    /// CPU% and RSS from each pane's process. Also returns the foreground
    /// command of the first pane in the first window.
    pub fn collect_session_stats(&self, session_name: &str) -> Result<(SessionStats, String)> {
        let windows = self.tmux.list_windows(session_name)?;

        let mut total_cpu: f32 = 0.0;
        let mut total_mem: u64 = 0;
        let mut foreground_command = String::new();

        for window in &windows {
            let panes = self
                .tmux
                .list_panes(session_name, &window.index.to_string())?;

            for pane in &panes {
                // Capture the first pane's command as the foreground command.
                if foreground_command.is_empty() {
                    foreground_command = pane.current_command.clone();
                }

                match self.sys_probe.process_stats(pane.pid) {
                    Ok(stats) => {
                        total_cpu += stats.cpu_percent;
                        total_mem += stats.memory_bytes;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to get process stats for pid {} in session {}: {e:#}",
                            pane.pid,
                            session_name
                        );
                    }
                }
            }
        }

        Ok((
            SessionStats {
                cpu_percent: total_cpu,
                memory_bytes: total_mem,
            },
            foreground_command,
        ))
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Build a complete [`MonitorEvent`] for one tick: fd usage + all sessions.
    async fn collect_event(&self) -> Result<MonitorEvent> {
        // --- File descriptor usage ---
        let (fd_current, fd_max) = self.sys_probe.fd_usage()?;
        let fd_percent = if fd_max == 0 {
            0u8
        } else {
            let raw = (fd_current * 100) / fd_max;
            if raw > 100 { 100u8 } else { raw as u8 }
        };

        // --- Per-session stats ---
        let raw_sessions = self.tmux.list_sessions()?;
        let mut sessions = Vec::with_capacity(raw_sessions.len());

        for raw in &raw_sessions {
            match self.collect_session_stats(&raw.name) {
                Ok((stats, fg_cmd)) => {
                    sessions.push(SessionStatus {
                        name: raw.name.clone(),
                        stats,
                        last_activity: raw.activity,
                        created: raw.created,
                        attached_clients: raw.attached_clients,
                        foreground_command: fg_cmd,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to collect stats for session '{}': {e:#}",
                        raw.name
                    );
                }
            }
        }

        Ok(MonitorEvent {
            fd_current,
            fd_max,
            fd_percent,
            sessions,
        })
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProcStats, RawPane, RawSession, RawWindow};
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // MockTmux
    // -----------------------------------------------------------------------

    struct MockTmux {
        sessions: Vec<RawSession>,
        windows: Vec<RawWindow>,
        panes: Vec<RawPane>,
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
        fn new_session(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        fn kill_session(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        fn kill_server(&self) -> Result<()> {
            Ok(())
        }
        fn start_server(&self) -> Result<()> {
            Ok(())
        }
        fn attach_session(&self, _name: &str) -> Result<()> {
            Ok(())
        }
        fn session_activity(&self, _session: &str) -> Result<i64> {
            Ok(0)
        }

        fn new_window(&self, _session: &str, _name: &str) -> anyhow::Result<()> { Ok(()) }
        fn split_window(&self, _session: &str, _window: &str) -> anyhow::Result<()> { Ok(()) }
        fn send_keys(&self, _target: &str, _keys: &str) -> anyhow::Result<()> { Ok(()) }
        fn select_layout(&self, _target: &str, _layout: &str) -> anyhow::Result<()> { Ok(()) }
        fn get_global_option(&self, _name: &str) -> anyhow::Result<String> {
            Ok("0".to_string())
        }
        fn rename_session(&self, _old_name: &str, _new_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // MockSysProbe
    // -----------------------------------------------------------------------

    struct MockSysProbe {
        fd_current: u64,
        fd_max: u64,
        proc_stats: ProcStats,
    }

    impl MockSysProbe {
        fn new(fd_current: u64, fd_max: u64, cpu: f32, mem: u64) -> Self {
            Self {
                fd_current,
                fd_max,
                proc_stats: ProcStats {
                    cpu_percent: cpu,
                    memory_bytes: mem,
                },
            }
        }
    }

    impl SystemProbe for MockSysProbe {
        fn fd_usage(&self) -> Result<(u64, u64)> {
            Ok((self.fd_current, self.fd_max))
        }
        fn process_stats(&self, _pid: u32) -> Result<ProcStats> {
            Ok(self.proc_stats.clone())
        }
    }

    // -----------------------------------------------------------------------
    // Helper: build service with mocks
    // -----------------------------------------------------------------------

    fn make_service(
        tmux: Arc<dyn TmuxAdapter>,
        sys_probe: Arc<dyn SystemProbe>,
    ) -> (MonitorService, broadcast::Receiver<MonitorEvent>) {
        MonitorService::new(tmux, sys_probe, 1, 16)
    }

    // -----------------------------------------------------------------------
    // collect_session_stats tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_collect_session_stats_aggregates_multiple_panes() {
        let tmux = Arc::new(MockTmux::new(
            vec![],
            vec![
                RawWindow { index: 0, name: "a".into(), layout: "l".into() },
                RawWindow { index: 1, name: "b".into(), layout: "l".into() },
            ],
            vec![
                RawPane { index: 0, pid: 100, current_dir: "/".into(), current_command: "sh".into() },
                RawPane { index: 1, pid: 101, current_dir: "/".into(), current_command: "vi".into() },
            ],
        ));
        // Each pane returns cpu=10.0, mem=1024
        let sys = Arc::new(MockSysProbe::new(0, 0, 10.0, 1024));
        let (svc, _rx) = make_service(tmux, sys);

        let (stats, fg_cmd) = svc.collect_session_stats("test").unwrap();

        // 2 windows x 2 panes each = 4 panes total
        assert!((stats.cpu_percent - 40.0).abs() < 0.01);
        assert_eq!(stats.memory_bytes, 4096);
        // First pane's command
        assert_eq!(fg_cmd, "sh");
    }

    #[test]
    fn test_collect_session_stats_empty_panes() {
        let tmux = Arc::new(MockTmux::new(
            vec![],
            vec![RawWindow { index: 0, name: "w".into(), layout: "l".into() }],
            vec![], // no panes
        ));
        let sys = Arc::new(MockSysProbe::new(0, 0, 99.0, 9999));
        let (svc, _rx) = make_service(tmux, sys);

        let (stats, fg_cmd) = svc.collect_session_stats("empty").unwrap();

        assert!((stats.cpu_percent - 0.0).abs() < 0.01);
        assert_eq!(stats.memory_bytes, 0);
        assert!(fg_cmd.is_empty());
    }

    // -----------------------------------------------------------------------
    // fd_percent tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_fd_percent_calculation() {
        let tmux = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let sys = Arc::new(MockSysProbe::new(500, 1000, 0.0, 0));
        let (svc, _rx) = make_service(tmux, sys);

        let event = svc.collect_event().await.unwrap();

        assert_eq!(event.fd_current, 500);
        assert_eq!(event.fd_max, 1000);
        assert_eq!(event.fd_percent, 50);
    }

    #[tokio::test]
    async fn test_fd_percent_max_zero_no_panic() {
        let tmux = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let sys = Arc::new(MockSysProbe::new(123, 0, 0.0, 0));
        let (svc, _rx) = make_service(tmux, sys);

        let event = svc.collect_event().await.unwrap();

        // When max is 0, fd_percent should be 0 (avoid division by zero).
        assert_eq!(event.fd_percent, 0);
    }

    // -----------------------------------------------------------------------
    // run() integration test — at least one event is received
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_run_sends_event() {
        let tmux = Arc::new(MockTmux::new(
            vec![RawSession {
                name: "s1".into(),
                created: 1_700_000_000,
                attached_clients: 1,
                activity: 1_700_001_000,
            }],
            vec![RawWindow { index: 0, name: "w".into(), layout: "l".into() }],
            vec![RawPane { index: 0, pid: 42, current_dir: "/".into(), current_command: "sh".into() }],
        ));
        let sys = Arc::new(MockSysProbe::new(200, 1000, 5.0, 2048));

        // Use a very short interval so we get a quick first tick.
        let (svc, mut rx) = MonitorService::new(tmux, sys, 1, 16);

        // Spawn the run loop and race against a timeout.
        let handle = tokio::spawn(async move { svc.run().await });

        let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("timed out waiting for MonitorEvent")
            .expect("broadcast recv error");

        // Verify the event contents.
        assert_eq!(event.fd_current, 200);
        assert_eq!(event.fd_max, 1000);
        assert_eq!(event.fd_percent, 20);
        assert_eq!(event.sessions.len(), 1);
        assert_eq!(event.sessions[0].name, "s1");
        assert!((event.sessions[0].stats.cpu_percent - 5.0).abs() < 0.01);
        assert_eq!(event.sessions[0].stats.memory_bytes, 2048);
        assert_eq!(event.sessions[0].last_activity, 1_700_001_000);

        handle.abort();
    }

    // -----------------------------------------------------------------------
    // subscribe() returns independent receiver
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_subscribe_returns_independent_receiver() {
        let tmux = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let sys = Arc::new(MockSysProbe::new(100, 200, 0.0, 0));
        let (svc, mut rx1) = MonitorService::new(tmux, sys, 1, 16);
        let mut rx2 = svc.subscribe();

        let handle = tokio::spawn(async move { svc.run().await });

        let e1 = tokio::time::timeout(Duration::from_secs(3), rx1.recv())
            .await
            .expect("timeout rx1")
            .expect("recv rx1");
        let e2 = tokio::time::timeout(Duration::from_secs(3), rx2.recv())
            .await
            .expect("timeout rx2")
            .expect("recv rx2");

        assert_eq!(e1.fd_percent, 50);
        assert_eq!(e2.fd_percent, 50);

        handle.abort();
    }

    // -----------------------------------------------------------------------
    // update_interval
    // -----------------------------------------------------------------------

    #[test]
    fn test_update_interval() {
        let tmux: Arc<dyn TmuxAdapter> = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let sys: Arc<dyn SystemProbe> = Arc::new(MockSysProbe::new(0, 0, 0.0, 0));
        let (mut svc, _rx) = MonitorService::new(tmux, sys, 3, 8);

        assert_eq!(svc.poll_interval, Duration::from_secs(3));

        svc.update_interval(10);
        assert_eq!(svc.poll_interval, Duration::from_secs(10));
    }
}
