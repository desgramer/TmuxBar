use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSColor, NSForegroundColorAttributeName, NSMenu, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSString};

use crate::models::AlertLevel;

/// Manages the macOS menu-bar status item (the small icon/text in the
/// top-right area of the screen). The appearance changes colour to reflect
/// the current [`AlertLevel`].
pub struct MenuBarApp {
    status_item: Retained<NSStatusItem>,
}

// ---------------------------------------------------------------------------
// Icon glyphs per alert level
// ---------------------------------------------------------------------------

fn icon_for_level(level: &AlertLevel) -> &'static str {
    match level {
        AlertLevel::Normal => "\u{25CF}",   // ●
        AlertLevel::Warning => "\u{26A0}",  // ⚠
        AlertLevel::Elevated => "\u{26A0}", // ⚠ (same glyph, different colour)
        AlertLevel::Critical => "\u{26D4}", // ⛔
    }
}

fn color_for_level(level: &AlertLevel) -> Retained<NSColor> {
    match level {
        AlertLevel::Normal => NSColor::systemGreenColor(),
        AlertLevel::Warning => NSColor::systemYellowColor(),
        AlertLevel::Elevated => NSColor::systemYellowColor(),
        AlertLevel::Critical => NSColor::systemRedColor(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an `NSAttributedString` with the given text and foreground colour.
fn colored_string(text: &str, color: &NSColor) -> Retained<NSAttributedString> {
    let ns_text = NSString::from_str(text);

    // Build a single-entry attributes dictionary: { NSForegroundColorAttributeName: color }
    let key: &NSString = unsafe { NSForegroundColorAttributeName };
    let attrs: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
        NSDictionary::from_slices(&[key], &[color]);

    // SAFETY: the attribute dictionary contains a valid NSForegroundColorAttributeName -> NSColor
    // mapping, which is the expected type for this key.
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &ns_text,
            Some(&attrs),
        )
    }
}

// ---------------------------------------------------------------------------
// MenuBarApp
// ---------------------------------------------------------------------------

impl MenuBarApp {
    /// Create a new status-bar item. Must be called on the main thread.
    pub fn new(mtm: MainThreadMarker) -> Self {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);

        let app = Self { status_item };
        app.set_alert_level(&AlertLevel::Normal, mtm);
        app
    }

    /// Update the icon/colour to reflect the current alert level.
    pub fn set_alert_level(&self, level: &AlertLevel, mtm: MainThreadMarker) {
        let icon = icon_for_level(level);
        let color = color_for_level(level);
        let attributed = colored_string(icon, &color);

        if let Some(button) = self.status_item.button(mtm) {
            button.setAttributedTitle(&attributed);
        }
    }

    /// Attach an `NSMenu` to the status item (shown on click).
    pub fn set_menu(&self, menu: &NSMenu) {
        self.status_item.setMenu(Some(menu));
    }

    /// Borrow the underlying `NSStatusItem` for further wiring.
    pub fn status_item(&self) -> &NSStatusItem {
        &self.status_item
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Compilation-only smoke test: ensures the module's types resolve.
    #[test]
    fn types_compile() {
        // NSStatusItem is *not* Send -- this is expected for AppKit objects.
        // We just verify the struct definition compiles.
        let _ = std::mem::size_of::<MenuBarApp>();
    }

    /// Verifies that helper functions return sensible values without needing
    /// a running NSApplication.
    #[test]
    fn icon_mapping() {
        assert_eq!(icon_for_level(&AlertLevel::Normal), "\u{25CF}");
        assert_eq!(icon_for_level(&AlertLevel::Warning), "\u{26A0}");
        assert_eq!(icon_for_level(&AlertLevel::Elevated), "\u{26A0}");
        assert_eq!(icon_for_level(&AlertLevel::Critical), "\u{26D4}");
    }

    /// Integration test that requires the main thread and a running
    /// NSApplication. Ignored by default -- run with `--ignored` on a Mac.
    #[test]
    #[ignore = "requires main thread and NSApplication event loop"]
    fn construct_on_main_thread() {
        let mtm = MainThreadMarker::new().expect("test must run on the main thread");
        let app = MenuBarApp::new(mtm);
        // If we get here, construction succeeded.
        let _ = app.status_item();
    }
}
