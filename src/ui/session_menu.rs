use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::i18n::{self, Language};
use crate::models::{Session, SessionStats};
use crate::ui::menu_action_handler::MenuActionHandler;

// ---------------------------------------------------------------------------
// Tag constants for menu item identification
// ---------------------------------------------------------------------------

/// Tags 0..999 are reserved for session items (index-based).
pub const TAG_NEW_SESSION: isize = 1000;
pub const TAG_KILL_SERVER: isize = 1001;
pub const TAG_SETTINGS: isize = 1002;
pub const TAG_QUIT: isize = 1003;
/// Tags 2000..2999 are reserved for session kill items.
pub const TAG_KILL_SESSION_BASE: isize = 2000;
/// Tags 3000..3999 are reserved for session rename items.
pub const TAG_RENAME_SESSION_BASE: isize = 3000;

// ---------------------------------------------------------------------------
// SessionMenuBuilder
// ---------------------------------------------------------------------------

/// Builds an `NSMenu` from a list of tmux sessions.
///
/// The menu is rebuilt each time the status bar icon is refreshed, using fresh
/// session data. Each item's target/action is wired to the provided
/// `MenuActionHandler` so that clicks dispatch `AppCommand`s.
pub struct SessionMenuBuilder;

impl SessionMenuBuilder {
    /// Build an `NSMenu` reflecting the current session list.
    ///
    /// Each session becomes a menu item with a formatted title and a tag equal
    /// to its index in `sessions`. Fixed action items ("New Session…", "Kill
    /// Server", "Settings", "Quit") use well-known tag constants.
    ///
    /// When `handler` is `Some`, every item gets a target/action pair so that
    /// clicks are routed to `MenuActionHandler::menu_item_clicked:`.
    pub fn build_menu(
        mtm: MainThreadMarker,
        sessions: &[Session],
        handler: Option<&MenuActionHandler>,
        lang: &Language,
    ) -> Retained<NSMenu> {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("TmuxBar"));

        // Keep the handler's name list in sync with the tag indices.
        if let Some(h) = handler {
            h.update_session_names(sessions.iter().map(|s| s.name.clone()).collect());
        }

        // --- Session items: click to attach, submenu for rename/kill ---
        for (idx, session) in sessions.iter().enumerate() {
            let title = format_session_title(session);
            // Parent item is clickable (attach action via tag 0..999).
            let session_item = make_item(mtm, &title, Some("terminal"), handler);
            session_item.setTag(idx as isize);
            session_item.setEnabled(true);

            // Submenu for destructive / edit actions only.
            let submenu =
                NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&session.name));

            let rename_item = make_item(mtm, i18n::menu_rename(lang), Some("pencil"), handler);
            rename_item.setTag(TAG_RENAME_SESSION_BASE + idx as isize);
            submenu.addItem(&rename_item);

            let kill_item = make_item(mtm, i18n::menu_kill_session(lang), Some("trash"), handler);
            kill_item.setTag(TAG_KILL_SESSION_BASE + idx as isize);
            submenu.addItem(&kill_item);

            session_item.setSubmenu(Some(&submenu));
            menu.addItem(&session_item);
        }

        // --- Separator after sessions (only if there are sessions) ---
        if !sessions.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }

        // --- Fixed action items ---
        let new_session = make_item(
            mtm,
            i18n::menu_new_session(lang),
            Some("plus.square"),
            handler,
        );
        new_session.setTag(TAG_NEW_SESSION);
        menu.addItem(&new_session);

        let kill_server = make_item(mtm, i18n::menu_kill_server(lang), Some("power"), handler);
        kill_server.setTag(TAG_KILL_SERVER);
        menu.addItem(&kill_server);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let settings = make_item(mtm, i18n::menu_settings(lang), Some("gearshape"), handler);
        settings.setTag(TAG_SETTINGS);
        menu.addItem(&settings);

        let quit = make_item(
            mtm,
            i18n::menu_quit(lang),
            Some("arrow.right.circle"),
            handler,
        );
        quit.setTag(TAG_QUIT);
        menu.addItem(&quit);

        menu
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Format a `chrono::Duration` as `"Xh Ym"`.
///
/// Negative or zero durations are rendered as `"0h 0m"`.
pub fn format_uptime(duration: &chrono::Duration) -> String {
    let total_secs = duration.num_seconds().max(0);
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    format!("{}h {}m", hours, minutes)
}

/// Format the complete title for a session menu item.
///
/// Pattern: `"<name> (<uptime>) — <command>"` with optional stats suffix.
pub fn format_session_title(session: &Session) -> String {
    let uptime = format_uptime(&session.uptime);
    let mut title = format!(
        "{} ({}) — {}",
        session.name, uptime, session.foreground_command
    );

    if let Some(ref stats) = session.stats {
        let stats_str = format_stats(stats);
        title.push_str("  ");
        title.push_str(&stats_str);
    }

    title
}

/// Format session statistics as `"CPU: X.X% | MEM: XMB"` or `"CPU: X.X% | MEM: X.XGB"`.
///
/// Memory is displayed in GB when >= 1 GB, otherwise in MB.
pub fn format_stats(stats: &SessionStats) -> String {
    let mem = format_memory(stats.memory_bytes);
    format!("CPU: {:.1}% | MEM: {}", stats.cpu_percent, mem)
}

/// Format a byte count as a human-readable memory string.
fn format_memory(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824; // 1024^3
    const MB: u64 = 1_048_576; // 1024^2

    if bytes >= GB {
        let gb = bytes as f64 / GB as f64;
        format!("{:.1}GB", gb)
    } else {
        let mb = bytes / MB;
        format!("{}MB", mb)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Create an `NSMenuItem` with the given title and optional action handler.
fn make_item(
    mtm: MainThreadMarker,
    title: &str,
    icon_name: Option<&str>,
    handler: Option<&MenuActionHandler>,
) -> Retained<NSMenuItem> {
    let ns_title = NSString::from_str(title);
    let empty = NSString::from_str("");
    let sel = handler.map(|_| MenuActionHandler::action_sel());

    // SAFETY: the selector (if set) matches MenuActionHandler's registered method.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &ns_title,
            sel,
            &empty,
        )
    };

    if let Some(name) = icon_name {
        let ns_icon_name = NSString::from_str(name);
        if let Some(image) =
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns_icon_name, Some(&empty))
        {
            item.setImage(Some(&image));
        }
    }

    if let Some(h) = handler {
        let target: &AnyObject = h;
        // SAFETY: target is a valid NSObject subclass that responds to the selector.
        unsafe { item.setTarget(Some(target)) };
    }

    item
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- format_uptime --------------------------------------------------------

    #[test]
    fn test_format_uptime_hours_and_minutes() {
        let d = chrono::Duration::seconds(2 * 3600 + 15 * 60);
        assert_eq!(format_uptime(&d), "2h 15m");
    }

    #[test]
    fn test_format_uptime_zero() {
        let d = chrono::Duration::seconds(0);
        assert_eq!(format_uptime(&d), "0h 0m");
    }

    #[test]
    fn test_format_uptime_minutes_only() {
        let d = chrono::Duration::seconds(30 * 60);
        assert_eq!(format_uptime(&d), "0h 30m");
    }

    #[test]
    fn test_format_uptime_large() {
        let d = chrono::Duration::seconds(48 * 3600 + 59 * 60 + 59);
        assert_eq!(format_uptime(&d), "48h 59m");
    }

    #[test]
    fn test_format_uptime_negative_clamps_to_zero() {
        let d = chrono::Duration::seconds(-100);
        assert_eq!(format_uptime(&d), "0h 0m");
    }

    // -- format_session_title -------------------------------------------------

    #[test]
    fn test_format_session_title_without_stats() {
        let session = Session {
            name: "dev".into(),
            uptime: chrono::Duration::seconds(2 * 3600 + 15 * 60),
            foreground_command: "vim".into(),
            attached_clients: 1,
            stats: None,
        };
        assert_eq!(format_session_title(&session), "dev (2h 15m) — vim");
    }

    #[test]
    fn test_format_session_title_with_stats() {
        let session = Session {
            name: "build".into(),
            uptime: chrono::Duration::seconds(30 * 60),
            foreground_command: "cargo".into(),
            attached_clients: 0,
            stats: Some(SessionStats {
                cpu_percent: 8.1,
                memory_bytes: 120 * 1_048_576, // 120 MB
            }),
        };
        assert_eq!(
            format_session_title(&session),
            "build (0h 30m) — cargo  CPU: 8.1% | MEM: 120MB"
        );
    }

    // -- format_stats ---------------------------------------------------------

    #[test]
    fn test_format_stats_megabytes() {
        let stats = SessionStats {
            cpu_percent: 12.3,
            memory_bytes: 45 * 1_048_576, // 45 MB
        };
        assert_eq!(format_stats(&stats), "CPU: 12.3% | MEM: 45MB");
    }

    #[test]
    fn test_format_stats_gigabytes() {
        let stats = SessionStats {
            cpu_percent: 5.0,
            memory_bytes: 2 * 1_073_741_824 + 536_870_912, // 2.5 GB
        };
        assert_eq!(format_stats(&stats), "CPU: 5.0% | MEM: 2.5GB");
    }

    #[test]
    fn test_format_stats_zero() {
        let stats = SessionStats {
            cpu_percent: 0.0,
            memory_bytes: 0,
        };
        assert_eq!(format_stats(&stats), "CPU: 0.0% | MEM: 0MB");
    }

    #[test]
    fn test_format_stats_exactly_one_gb() {
        let stats = SessionStats {
            cpu_percent: 99.9,
            memory_bytes: 1_073_741_824, // exactly 1 GB
        };
        assert_eq!(format_stats(&stats), "CPU: 99.9% | MEM: 1.0GB");
    }

    // -- format_memory (indirect) ---------------------------------------------

    #[test]
    fn test_format_memory_sub_megabyte() {
        // Less than 1 MB rounds to 0 MB
        let stats = SessionStats {
            cpu_percent: 1.0,
            memory_bytes: 500_000,
        };
        assert_eq!(format_stats(&stats), "CPU: 1.0% | MEM: 0MB");
    }

    // -- Tag constants --------------------------------------------------------

    #[test]
    fn test_tag_constants_are_distinct() {
        let tags = [
            TAG_NEW_SESSION,
            TAG_KILL_SERVER,
            TAG_SETTINGS,
            TAG_QUIT,
            TAG_KILL_SESSION_BASE,
            TAG_RENAME_SESSION_BASE,
        ];
        for (i, a) in tags.iter().enumerate() {
            for (j, b) in tags.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "tags at index {} and {} must differ", i, j);
                }
            }
        }
    }

    #[test]
    fn test_tag_constants_do_not_overlap_session_range() {
        // Session tags occupy 0..999
        assert!(TAG_NEW_SESSION >= 1000);
        assert!(TAG_KILL_SERVER >= 1000);
        assert!(TAG_SETTINGS >= 1000);
        assert!(TAG_QUIT >= 1000);
    }

    #[test]
    fn test_kill_session_tag_range_does_not_overlap() {
        assert!(TAG_KILL_SESSION_BASE >= 2000);
        assert!(TAG_NEW_SESSION < TAG_KILL_SESSION_BASE);
    }

    #[test]
    fn test_rename_session_tag_range_does_not_overlap() {
        assert!(TAG_RENAME_SESSION_BASE >= 3000);
        assert!(TAG_KILL_SESSION_BASE < TAG_RENAME_SESSION_BASE);
    }
}
