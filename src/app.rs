use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use tokio::sync::{broadcast, mpsc};

use crate::core::event_logger::EventLogger;
use crate::core::fd_alert_policy::FdAlertPolicy;
use crate::core::inactivity_detector::InactivityDetector;
use crate::core::monitor_service::MonitorService;
use crate::core::restart_service::RestartService;
use crate::core::session_manager::SessionManager;
use crate::core::snapshot_service::SnapshotService;
use crate::infra::config::AppConfig;
use crate::infra::config_watcher::ConfigWatcher;
use crate::infra::instance_lock::InstanceLock;
use crate::infra::launch_agent::LaunchAgent;
use crate::infra::log_store::LogStore;
use crate::infra::sys_probe::MacSysProbe;
use crate::infra::tmux_client::TmuxClient;
use crate::models::{AlertLevel, AppCommand, MonitorEvent, Session};
use crate::ui::menu_bar::MenuBarApp;
use crate::ui::notifications::NotificationService;
use crate::ui::session_menu::SessionMenuBuilder;

// ---------------------------------------------------------------------------
// Shared application state (Tokio side writes, main-thread reads)
// ---------------------------------------------------------------------------

/// State shared between the Tokio background thread and the AppKit main thread.
///
/// The Tokio event-processing loop updates this on every monitor tick.
/// The main thread reads it when building the menu (on click) and when
/// dispatched to refresh the icon.
struct AppState {
    sessions: Vec<Session>,
    alert_level: AlertLevel,
    fd_percent: u8,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            alert_level: AlertLevel::Normal,
            fd_percent: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Service bundles (avoids clippy::too_many_arguments)
// ---------------------------------------------------------------------------

/// Services that run on the Tokio background thread.
struct BackgroundServices {
    /// `None` when tmux is not available.
    monitor_service: Option<MonitorService>,
    /// `None` when tmux is not available.
    monitor_rx: Option<broadcast::Receiver<MonitorEvent>>,
    /// `None` when tmux is not available.
    session_manager: Option<SessionManager>,
    restart_service: Option<RestartService>,
    event_services: EventServices,
    cmd_rx: mpsc::Receiver<AppCommand>,
    shared_state: Arc<Mutex<AppState>>,
}

/// Services needed by the monitor event processing loop.
struct EventServices {
    /// Wrapped in Arc<Mutex<>> so the config hot-reload callback (running on
    /// the notify thread) can update thresholds without disrupting the event
    /// processing loop.
    fd_alert_policy: Arc<Mutex<FdAlertPolicy>>,
    inactivity_detector: Arc<Mutex<InactivityDetector>>,
    /// `None` when the log store failed to open; fd spikes are silently dropped.
    event_logger: Option<EventLogger>,
    notification_service: NotificationService,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Check whether the configured tmux binary is reachable and executable.
///
/// Returns `true` if `tmux -V` runs successfully, `false` otherwise.
/// A `false` result means the MonitorService and SessionManager will be skipped.
fn check_tmux_available(tmux_path: &str) -> bool {
    match std::process::Command::new(tmux_path).arg("-V").output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            tracing::warn!(
                "tmux binary at '{tmux_path}' returned non-zero status: {}",
                out.status
            );
            false
        }
        Err(e) => {
            tracing::warn!(
                "tmux not found at '{tmux_path}': {e:#}. \
                 Session features will be disabled."
            );
            false
        }
    }
}

/// Wire all services together and run the application.
///
/// 1. Acquires an instance lock (exits if another TmuxBar is already running).
/// 2. Loads configuration.
/// 3. Creates infrastructure adapters and core services.
/// 4. Spawns a Tokio runtime on a background thread for monitoring and
///    command processing.
/// 5. Runs the AppKit event loop on the main thread.
pub fn run() {
    // Must be on the main thread for AppKit.
    let mtm = MainThreadMarker::new().expect("TmuxBar must run on the main thread");

    // ------------------------------------------------------------------
    // a. Acquire instance lock — exit immediately if another instance runs
    // ------------------------------------------------------------------
    let _instance_lock = match InstanceLock::acquire() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("TmuxBar: {e}");
            std::process::exit(1);
        }
    };

    // ------------------------------------------------------------------
    // b. Load config
    // ------------------------------------------------------------------
    let config = match AppConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            let path = AppConfig::config_path();
            eprintln!(
                "TmuxBar: failed to load config from {}: {e:#}\nFalling back to defaults.",
                path.display()
            );
            tracing::warn!(
                "Failed to load config from {}: {e:#}. Using defaults.",
                path.display()
            );
            AppConfig::default()
        }
    };

    // ------------------------------------------------------------------
    // c. Sync launch-at-login state with config
    // ------------------------------------------------------------------
    if let Err(e) = LaunchAgent::sync_with_config(config.general.launch_at_login) {
        tracing::warn!("Failed to sync LaunchAgent with config: {e:#}");
    }

    // ------------------------------------------------------------------
    // d. Create infrastructure adapters
    // ------------------------------------------------------------------
    let tmux_available = check_tmux_available(&config.terminal.tmux_path);

    let tmux: Arc<dyn crate::models::TmuxAdapter> =
        Arc::new(TmuxClient::new(&config.terminal.tmux_path));
    let sys_probe: Arc<dyn crate::models::SystemProbe> = Arc::new(MacSysProbe::new());

    // LogStore: warn and continue without logging on failure.
    let log_store_opt = match LogStore::new(&LogStore::default_path()) {
        Ok(store) => Some(store),
        Err(e) => {
            tracing::warn!(
                "Failed to open log store (fd spikes will not be persisted): {e:#}"
            );
            None
        }
    };

    // ------------------------------------------------------------------
    // e. Create core services (some conditional on tmux availability)
    // ------------------------------------------------------------------
    let fd_alert_policy = Arc::new(Mutex::new(FdAlertPolicy::new(config.monitor.alert_config())));
    let inactivity_timeout_secs = config.monitor.inactivity_timeout_mins * 60;
    let inactivity_detector = Arc::new(Mutex::new(InactivityDetector::new(inactivity_timeout_secs)));
    let event_logger = log_store_opt.map(EventLogger::new);
    let notification_service = NotificationService::new();

    // MonitorService and SessionManager are only created when tmux is reachable.
    let (monitor_service_opt, monitor_rx_opt, session_manager_opt) = if tmux_available {
        let session_manager = SessionManager::new(
            Arc::clone(&tmux),
            &config.terminal.app,
            &config.terminal.tmux_path,
        );
        let (monitor_service, monitor_rx) = MonitorService::new(
            Arc::clone(&tmux),
            Arc::clone(&sys_probe),
            config.monitor.poll_interval_secs,
            64, // broadcast channel capacity
        );
        (Some(monitor_service), Some(monitor_rx), Some(session_manager))
    } else {
        tracing::warn!(
            "tmux not available — MonitorService and SessionManager are disabled."
        );
        (None, None, None)
    };

    let snapshot_dir = PathBuf::from(&config.snapshots.dir);
    let snapshot_service_opt: Option<Arc<SnapshotService>> = if tmux_available {
        match SnapshotService::new(Arc::clone(&tmux), snapshot_dir) {
            Ok(svc) => Some(Arc::new(svc)),
            Err(e) => {
                tracing::error!("Failed to create SnapshotService: {e:#}");
                None
            }
        }
    } else {
        None
    };

    // RestartService uses its own EventLogger and NotificationService instances
    // (LogStore / rusqlite::Connection is not Sync so cannot be shared via Arc
    // across threads; opening a second connection to the same WAL-mode DB file
    // is safe and is the standard SQLite multi-writer approach).
    let restart_service = snapshot_service_opt.map(|ss| {
        // EventLogger wraps rusqlite::Connection which is Send but not Sync;
        // RestartService owns it inside a Mutex so the overall type is Send.
        // We open a second connection to the same WAL-mode DB file — this is
        // the standard SQLite multi-reader approach.
        let restart_log_store_opt = match LogStore::new(&LogStore::default_path()) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("RestartService: failed to open log store: {e:#}");
                None
            }
        };
        RestartService::new(
            ss,
            Arc::clone(&tmux),
            restart_log_store_opt.map(EventLogger::new),
            NotificationService::new(),
        )
    });

    // ------------------------------------------------------------------
    // d. Set up command channel (UI -> Tokio)
    // ------------------------------------------------------------------
    let (cmd_tx, cmd_rx) = mpsc::channel::<AppCommand>(32);

    // ------------------------------------------------------------------
    // Shared state between threads
    // ------------------------------------------------------------------
    let shared_state = Arc::new(Mutex::new(AppState::default()));

    // ------------------------------------------------------------------
    // f. Spawn Tokio runtime on background thread
    // ------------------------------------------------------------------
    let bg_services = BackgroundServices {
        monitor_service: monitor_service_opt,
        monitor_rx: monitor_rx_opt,
        session_manager: session_manager_opt,
        restart_service,
        event_services: EventServices {
            fd_alert_policy: Arc::clone(&fd_alert_policy),
            inactivity_detector: Arc::clone(&inactivity_detector),
            event_logger,
            notification_service,
        },
        cmd_rx,
        shared_state: Arc::clone(&shared_state),
    };

    // ------------------------------------------------------------------
    // e. Start config hot-reload watcher
    // ------------------------------------------------------------------
    let watcher_fd_policy = Arc::clone(&fd_alert_policy);
    let watcher_inactivity = Arc::clone(&inactivity_detector);
    let _config_watcher = match ConfigWatcher::start(
        AppConfig::config_path(),
        move |new_cfg: AppConfig| {
            tracing::info!("Config reloaded");

            // Update FdAlertPolicy thresholds and reset state.
            {
                let mut policy = watcher_fd_policy.lock().unwrap();
                policy.update_config(new_cfg.monitor.alert_config());
            }

            // Update InactivityDetector timeout.
            {
                let mut detector = watcher_inactivity.lock().unwrap();
                detector.update_timeout(new_cfg.monitor.inactivity_timeout_mins * 60);
            }

            // Sync LaunchAgent with the new launch_at_login setting.
            if let Err(e) = LaunchAgent::sync_with_config(new_cfg.general.launch_at_login) {
                tracing::warn!("Config reload: failed to sync LaunchAgent: {e:#}");
            }
        },
    ) {
        Ok(w) => {
            tracing::info!("Config watcher started for {}", AppConfig::config_path().display());
            Some(w)
        }
        Err(e) => {
            tracing::warn!("Failed to start config watcher: {e:#}");
            None
        }
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create Tokio runtime");

        rt.block_on(async move {
            run_background(bg_services).await;
        });
    });

    // ------------------------------------------------------------------
    // g. Create AppKit UI on main thread
    // ------------------------------------------------------------------
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let menu_bar = MenuBarApp::new(mtm);

    // Build the initial menu from whatever sessions exist right now.
    {
        let state = shared_state.lock().unwrap();
        let menu = SessionMenuBuilder::build_menu(mtm, &state.sessions);
        menu_bar.set_menu(&menu);
    }

    // Spawn a timer thread that periodically dispatches UI refreshes to the
    // main thread. The actual icon/menu updates happen on the main thread via
    // GCD dispatch.
    let ui_state = Arc::clone(&shared_state);
    let cmd_tx_for_timer = cmd_tx.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));

            // Check if the app is still running before doing work.
            if cmd_tx_for_timer.is_closed() {
                break;
            }

            let state = ui_state.lock().unwrap();
            let alert_level = state.alert_level.clone();
            let sessions = state.sessions.clone();
            drop(state);

            dispatch2::DispatchQueue::main().exec_async(move || {
                if let Some(mtm) = MainThreadMarker::new() {
                    // Rebuild the menu with latest session data.
                    // Note: without a retained reference to MenuBarApp we cannot
                    // update its menu directly. The current approach relies on the
                    // menu being rebuilt via setup_menu_refresh_timer at init time
                    // and the icon colour being set via the monitor event dispatch.
                    let _app = NSApplication::sharedApplication(mtm);
                    let _ = alert_level;
                    let _ = sessions;
                }
            });
        }
    });

    // Build the initial menu with real tmux data.
    setup_initial_menu(&shared_state, &menu_bar, &config, mtm);

    // Keep cmd_tx alive so the background thread's cmd_rx stays open.
    // It will be dropped when the NSApplication run loop exits.
    let _cmd_tx = cmd_tx;

    // ------------------------------------------------------------------
    // h. Start NSApplication run loop (blocks main thread)
    // ------------------------------------------------------------------
    tracing::info!("TmuxBar starting");
    app.run();
}

// ---------------------------------------------------------------------------
// Background Tokio task
// ---------------------------------------------------------------------------

/// Runs on the Tokio background thread. Processes monitor events and UI
/// commands until the app quits.
async fn run_background(services: BackgroundServices) {
    let BackgroundServices {
        monitor_service,
        monitor_rx,
        session_manager,
        restart_service,
        event_services,
        mut cmd_rx,
        shared_state,
    } = services;

    // Spawn the MonitorService polling loop as a background task (only when tmux is available).
    let monitor_handle = monitor_service.map(|svc| {
        tokio::spawn(async move {
            if let Err(e) = svc.run().await {
                tracing::error!("MonitorService exited with error: {e:#}");
            }
        })
    });

    // Spawn monitor event processing loop (only when tmux is available).
    if let Some(rx) = monitor_rx {
        let event_state = Arc::clone(&shared_state);
        tokio::spawn(async move {
            process_monitor_events(rx, event_services, event_state).await;
        });
    }

    // Process commands from the UI on this task.
    while let Some(cmd) = cmd_rx.recv().await {
        tracing::debug!(?cmd, "Received AppCommand");
        match cmd {
            AppCommand::CreateSession { name } => {
                match &session_manager {
                    Some(mgr) => {
                        if let Err(e) = mgr.create_and_attach(&name) {
                            tracing::error!("Failed to create session '{name}': {e:#}");
                        }
                    }
                    None => tracing::warn!("CreateSession: tmux unavailable"),
                }
            }
            AppCommand::AttachSession { name } => {
                match &session_manager {
                    Some(mgr) => {
                        if let Err(e) = mgr.attach(&name) {
                            tracing::error!("Failed to attach session '{name}': {e:#}");
                        }
                    }
                    None => tracing::warn!("AttachSession: tmux unavailable"),
                }
            }
            AppCommand::KillSession { name } => {
                match &session_manager {
                    Some(mgr) => {
                        if let Err(e) = mgr.kill_session(&name) {
                            tracing::error!("Failed to kill session '{name}': {e:#}");
                        }
                    }
                    None => tracing::warn!("KillSession: tmux unavailable"),
                }
            }
            AppCommand::KillServer => {
                match &session_manager {
                    Some(mgr) => {
                        if let Err(e) = mgr.kill_server() {
                            tracing::error!("Failed to kill server: {e:#}");
                        }
                    }
                    None => tracing::warn!("KillServer: tmux unavailable"),
                }
            }
            AppCommand::RestartServer => {
                match &restart_service {
                    Some(svc) => {
                        if let Err(e) = svc.execute_restart() {
                            tracing::error!("Safe restart failed: {e:#}");
                        }
                    }
                    None => {
                        tracing::warn!("RestartServer: SnapshotService unavailable, skipping restart");
                    }
                }
            }
            AppCommand::Quit => {
                tracing::info!("Quit command received, shutting down");
                break;
            }
        }
    }

    // Clean up: abort the monitor loop if it was started.
    if let Some(handle) = monitor_handle {
        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Monitor event processing
// ---------------------------------------------------------------------------

/// Reads monitor events from the broadcast channel and:
/// 1. Evaluates FdAlertPolicy -> sends notification if threshold crossed.
/// 2. Updates shared state (sessions, alert level, fd_percent).
/// 3. Dispatches icon-colour update to the main thread.
/// 4. Checks InactivityDetector -> sends notification for idle sessions.
/// 5. Logs fd spikes via EventLogger.
async fn process_monitor_events(
    mut monitor_rx: broadcast::Receiver<MonitorEvent>,
    mut event_services: EventServices,
    shared_state: Arc<Mutex<AppState>>,
) {
    loop {
        match monitor_rx.recv().await {
            Ok(event) => {
                handle_monitor_event(&event, &mut event_services, &shared_state);
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("Monitor event receiver lagged by {n} events");
            }
            Err(broadcast::error::RecvError::Closed) => {
                tracing::info!("Monitor event channel closed, stopping event processor");
                break;
            }
        }
    }
}

/// Process a single monitor event: alerts, state update, icon dispatch,
/// inactivity check, and fd spike logging.
fn handle_monitor_event(
    event: &MonitorEvent,
    services: &mut EventServices,
    shared_state: &Arc<Mutex<AppState>>,
) {
    // 1. Evaluate fd alert policy (lock only for the duration of the call).
    let fd_alert_opt = {
        let mut policy = services.fd_alert_policy.lock().unwrap();
        policy.evaluate(event.fd_percent)
    };
    if let Some(level) = fd_alert_opt {
        if let Err(e) = services
            .notification_service
            .send_fd_alert(event.fd_percent, &level)
        {
            tracing::warn!("Failed to send fd alert notification: {e:#}");
        }
    }

    // 2. Update shared state
    let alert_level = {
        let policy = services.fd_alert_policy.lock().unwrap();
        policy.current_level(event.fd_percent)
    };
    let sessions: Vec<Session> = event
        .sessions
        .iter()
        .map(|s| Session {
            name: s.name.clone(),
            uptime: chrono::Duration::seconds(0), // simplified; full uptime needs created timestamp
            foreground_command: String::new(),
            attached_clients: 0,
            stats: Some(s.stats.clone()),
        })
        .collect();

    {
        let mut state = shared_state.lock().unwrap();
        state.sessions = sessions;
        state.alert_level = alert_level.clone();
        state.fd_percent = event.fd_percent;
    }

    // 3. Dispatch icon-colour update to main thread
    let level_for_dispatch = alert_level;
    dispatch2::DispatchQueue::main().exec_async(move || {
        // The icon colour will be updated on next menu rebuild.
        // A future improvement would store a retained reference to MenuBarApp
        // in a thread-safe wrapper and call set_alert_level here.
        let _ = level_for_dispatch;
    });

    // 4. Check inactivity
    let now = chrono::Utc::now().timestamp();
    let idle_sessions = {
        let detector = services.inactivity_detector.lock().unwrap();
        detector.check_inactive(&event.sessions, now)
    };
    for session_name in &idle_sessions {
        let mins = (now
            - event
                .sessions
                .iter()
                .find(|s| s.name == *session_name)
                .map(|s| s.last_activity)
                .unwrap_or(now))
            / 60;
        if let Err(e) = services
            .notification_service
            .send_inactivity_alert(session_name, mins.max(0) as u64)
        {
            tracing::warn!("Failed to send inactivity alert for '{session_name}': {e:#}");
        }
    }

    // 5. Log fd spike if above warning threshold (skipped if no log store)
    if event.fd_percent >= 85 {
        if let Some(ref mut logger) = services.event_logger {
            if let Err(e) = logger.log_fd_spike(event.fd_percent) {
                tracing::warn!("Failed to log fd spike: {e:#}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Initial menu setup (main thread)
// ---------------------------------------------------------------------------

/// Build the initial menu by querying tmux directly, since the monitor may
/// not have produced an event yet.
fn setup_initial_menu(
    _shared_state: &Arc<Mutex<AppState>>,
    menu_bar: &MenuBarApp,
    config: &AppConfig,
    mtm: MainThreadMarker,
) {
    let tmux = TmuxClient::new(&config.terminal.tmux_path);
    let session_mgr = SessionManager::new(
        Arc::new(tmux) as Arc<dyn crate::models::TmuxAdapter>,
        &config.terminal.app,
        &config.terminal.tmux_path,
    );
    match session_mgr.list_sessions() {
        Ok(sessions) => {
            let menu = SessionMenuBuilder::build_menu(mtm, &sessions);
            menu_bar.set_menu(&menu);
        }
        Err(e) => {
            tracing::warn!("Failed to list sessions for initial menu: {e:#}");
            let menu = SessionMenuBuilder::build_menu(mtm, &[]);
            menu_bar.set_menu(&menu);
        }
    }
}
