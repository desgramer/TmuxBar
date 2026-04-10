use anyhow::Result;
use tracing::warn;

use crate::i18n::{self, Language};
use crate::models::AlertLevel;

// ---------------------------------------------------------------------------
// Public formatting helpers (tested independently of side-effecting dispatch)
// ---------------------------------------------------------------------------

/// Returns `(subtitle, body)` for an fd alert notification, or `None` for `Normal`.
pub(crate) fn format_fd_alert_message(
    pct: u8,
    level: &AlertLevel,
    lang: &Language,
) -> Option<(String, String)> {
    match level {
        AlertLevel::Normal => None,
        AlertLevel::Warning => Some((
            i18n::notif_fd_title(lang).to_string(),
            i18n::notif_fd_warn(lang, pct),
        )),
        AlertLevel::Elevated => Some((
            i18n::notif_fd_title(lang).to_string(),
            i18n::notif_fd_elevated(lang, pct),
        )),
        AlertLevel::Critical => Some((
            i18n::notif_fd_title(lang).to_string(),
            i18n::notif_fd_critical(lang, pct),
        )),
    }
}

/// Returns `(subtitle, body)` for an inactivity alert notification.
pub(crate) fn format_inactivity_message(
    session_name: &str,
    mins: u64,
    lang: &Language,
) -> (String, String) {
    (
        i18n::notif_inactivity_title(lang).to_string(),
        i18n::notif_inactivity_body(lang, session_name, mins),
    )
}

/// Returns `(subtitle, body)` for a restart-result notification.
pub(crate) fn format_restart_result_message(
    success: bool,
    details: &str,
    lang: &Language,
) -> (String, String) {
    if success {
        (
            i18n::notif_restart_success_title(lang).to_string(),
            i18n::notif_restart_success_body(lang, details),
        )
    } else {
        (
            i18n::notif_restart_fail_title(lang).to_string(),
            i18n::notif_restart_fail_body(lang, details),
        )
    }
}

// ---------------------------------------------------------------------------
// Bundle check — UNUserNotificationCenter requires a full .app bundle structure,
// not just an embedded Info.plist. Raw binaries fall back to osascript.
// ---------------------------------------------------------------------------

fn is_app_bundle() -> bool {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// NotificationService
// ---------------------------------------------------------------------------

pub struct NotificationService {
    use_native: bool,
}

impl NotificationService {
    /// Create a new `NotificationService`.
    ///
    /// Uses `UNUserNotificationCenter` if the process has a bundle identifier
    /// (i.e., running as a .app bundle). Falls back to `osascript` otherwise.
    pub fn new() -> Self {
        let use_native = is_app_bundle();
        if use_native {
            Self::request_authorization();
        }
        Self { use_native }
    }

    /// Request notification authorization (fire-and-forget).
    fn request_authorization() {
        use block2::RcBlock;
        use objc2::runtime::Bool;
        use objc2_foundation::NSError;
        use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};

        let center = UNUserNotificationCenter::currentNotificationCenter();
        let handler = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &handler,
        );
    }

    /// Send an fd-usage alert notification for the given percentage and level.
    ///
    /// Returns `Ok(())` immediately for `AlertLevel::Normal` (no notification sent).
    pub fn send_fd_alert(&self, pct: u8, level: &AlertLevel, lang: &Language) -> Result<()> {
        let Some((subtitle, body)) = format_fd_alert_message(pct, level, lang) else {
            return Ok(());
        };
        self.send_notification("TmuxBar", &subtitle, &body)
    }

    /// Send a session-inactivity alert notification.
    pub fn send_inactivity_alert(
        &self,
        session_name: &str,
        mins: u64,
        lang: &Language,
    ) -> Result<()> {
        let (subtitle, body) = format_inactivity_message(session_name, mins, lang);
        self.send_notification("TmuxBar", &subtitle, &body)
    }

    /// Send a notification reporting the result of a tmux server restart.
    pub fn send_restart_result(&self, success: bool, details: &str, lang: &Language) -> Result<()> {
        let (subtitle, body) = format_restart_result_message(success, details, lang);
        self.send_notification("TmuxBar", &subtitle, &body)
    }

    // -----------------------------------------------------------------------
    // Private dispatch
    // -----------------------------------------------------------------------

    pub(crate) fn send_notification(
        &self,
        title: &str,
        subtitle: &str,
        message: &str,
    ) -> Result<()> {
        if self.use_native {
            self.send_native(title, subtitle, message)
        } else {
            self.send_osascript(title, subtitle, message)
        }
    }

    /// Dispatch via `UNUserNotificationCenter` (native macOS notifications).
    fn send_native(&self, title: &str, subtitle: &str, message: &str) -> Result<()> {
        use block2::RcBlock;
        use objc2_foundation::{NSError, NSString};
        use objc2_user_notifications::{
            UNMutableNotificationContent, UNNotificationRequest, UNUserNotificationCenter,
        };

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setSubtitle(&NSString::from_str(subtitle));
        content.setBody(&NSString::from_str(message));

        let identifier = NSString::from_str(&format!(
            "tmuxbar-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));

        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );

        let center = UNUserNotificationCenter::currentNotificationCenter();
        let handler = RcBlock::new(|error: *mut NSError| {
            if !error.is_null() {
                tracing::warn!("UNUserNotificationCenter: failed to deliver notification");
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));

        Ok(())
    }

    /// Fallback: dispatch via `osascript` (opens Script Editor on click).
    fn send_osascript(&self, title: &str, subtitle: &str, message: &str) -> Result<()> {
        let title_esc = title.replace('"', "\\\"");
        let subtitle_esc = subtitle.replace('"', "\\\"");
        let message_esc = message.replace('"', "\\\"");

        let script = format!(
            r#"display notification "{message_esc}" with title "{title_esc}" subtitle "{subtitle_esc}""#
        );

        let status = std::process::Command::new("osascript")
            .args(["-e", &script])
            .status();

        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                warn!(
                    exit_code = ?s.code(),
                    "osascript exited with non-zero status; notification may not have been shown"
                );
            }
            Err(e) => {
                warn!(error = %e, "failed to spawn osascript; notification skipped");
            }
        }

        Ok(())
    }
}

impl Default for NotificationService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    // --- format_fd_alert_message ---

    #[test]
    fn normal_level_returns_none() {
        assert_eq!(
            format_fd_alert_message(50, &AlertLevel::Normal, &Language::En),
            None
        );
        assert_eq!(
            format_fd_alert_message(0, &AlertLevel::Normal, &Language::En),
            None
        );
        assert_eq!(
            format_fd_alert_message(100, &AlertLevel::Normal, &Language::En),
            None
        );
    }

    #[test]
    fn warning_message_contains_pct() {
        let result = format_fd_alert_message(85, &AlertLevel::Warning, &Language::En);
        let (subtitle, body) = result.expect("Warning should produce a message");
        assert!(body.contains("85%"), "body should mention 85%: {body}");
        assert!(!subtitle.is_empty());
    }

    #[test]
    fn elevated_message_contains_pct_and_warning_symbol() {
        let result = format_fd_alert_message(92, &AlertLevel::Elevated, &Language::En);
        let (subtitle, body) = result.expect("Elevated should produce a message");
        assert!(body.contains("92%"), "body should mention 92%: {body}");
        assert!(
            body.contains('⚠'),
            "body should contain warning symbol: {body}"
        );
        assert!(!subtitle.is_empty());
    }

    #[test]
    fn critical_message_contains_pct_and_critical_indicator() {
        let result = format_fd_alert_message(97, &AlertLevel::Critical, &Language::En);
        let (subtitle, body) = result.expect("Critical should produce a message");
        assert!(body.contains("97%"), "body should mention 97%: {body}");
        assert!(
            body.contains('🔴'),
            "body should contain critical indicator: {body}"
        );
        assert!(!subtitle.is_empty());
    }

    #[test]
    fn critical_message_suggests_restart() {
        let (_subtitle, body) = format_fd_alert_message(99, &AlertLevel::Critical, &Language::En)
            .expect("Critical should produce a message");
        assert!(
            body.to_lowercase().contains("restart"),
            "Critical body should suggest restarting: {body}"
        );
    }

    // --- format_inactivity_message ---

    #[test]
    fn inactivity_message_contains_session_and_minutes() {
        let (subtitle, body) = format_inactivity_message("my-session", 42, &Language::En);
        assert!(
            body.contains("my-session"),
            "body should contain session name: {body}"
        );
        assert!(
            body.contains("42"),
            "body should contain minute count: {body}"
        );
        assert!(!subtitle.is_empty());
    }

    #[test]
    fn inactivity_message_zero_minutes() {
        let (_subtitle, body) = format_inactivity_message("work", 0, &Language::En);
        assert!(
            body.contains("0 minutes"),
            "body should say 0 minutes: {body}"
        );
    }

    // --- format_restart_result_message ---

    #[test]
    fn restart_success_message_contains_details() {
        let (subtitle, body) =
            format_restart_result_message(true, "3 sessions restored", &Language::En);
        assert!(
            body.contains("3 sessions restored"),
            "body should include details: {body}"
        );
        assert!(
            body.to_lowercase().contains("success") || subtitle.to_lowercase().contains("success"),
            "should mention success: subtitle={subtitle}, body={body}"
        );
    }

    #[test]
    fn restart_failure_message_contains_details() {
        let (subtitle, body) = format_restart_result_message(false, "timeout", &Language::En);
        assert!(
            body.contains("timeout"),
            "body should include details: {body}"
        );
        assert!(
            body.to_lowercase().contains("fail") || subtitle.to_lowercase().contains("fail"),
            "should mention failure: subtitle={subtitle}, body={body}"
        );
    }

    #[test]
    fn restart_success_and_failure_are_different() {
        let (_, success_body) = format_restart_result_message(true, "ok", &Language::En);
        let (_, failure_body) = format_restart_result_message(false, "ok", &Language::En);
        assert_ne!(success_body, failure_body);
    }

    // --- NotificationService construction ---

    #[test]
    fn notification_service_constructs() {
        let _svc = NotificationService::new();
        let _svc2 = NotificationService::default();
    }

    #[test]
    fn send_fd_alert_normal_is_ok() {
        let svc = NotificationService::new();
        let result = svc.send_fd_alert(50, &AlertLevel::Normal, &Language::En);
        assert!(result.is_ok());
    }
}
