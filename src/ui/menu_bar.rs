use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSColor, NSImage, NSImageSymbolConfiguration, NSMenu, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::NSString;

use crate::models::AlertLevel;

/// Manages the macOS menu-bar status item (the small icon/text in the
/// top-right area of the screen). The appearance changes colour to reflect
/// the current [`AlertLevel`].
pub struct MenuBarApp {
    status_item: Retained<NSStatusItem>,
}

// ---------------------------------------------------------------------------
// Icon image per alert level
// ---------------------------------------------------------------------------

fn image_for_level(level: &AlertLevel) -> Retained<NSImage> {
    let symbol_name = match level {
        AlertLevel::Normal => "terminal",
        AlertLevel::Warning | AlertLevel::Elevated => "exclamationmark.triangle.fill",
        AlertLevel::Critical => "xmark.octagon.fill",
    };
    let ns_str = NSString::from_str(symbol_name);
    let empty = NSString::from_str("");

    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns_str, Some(&empty))
        .expect("Invalid SF Symbol name");

    match level {
        AlertLevel::Normal => {
            image.setTemplate(true);
            image
        }
        _ => {
            let color = match level {
                AlertLevel::Warning | AlertLevel::Elevated => NSColor::systemYellowColor(),
                AlertLevel::Critical => NSColor::systemRedColor(),
                _ => unreachable!(),
            };

            let config = NSImageSymbolConfiguration::configurationWithHierarchicalColor(&color);
            let image_with_config = image
                .imageWithSymbolConfiguration(&config)
                .unwrap_or(image.clone());

            image_with_config.setTemplate(false);
            image_with_config
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone helpers (accessible from app.rs)
// ---------------------------------------------------------------------------

/// Apply an alert-level change to a raw `NSStatusItem` pointer.
///
/// # Safety
///
/// `raw_ptr` must be a valid pointer to an `NSStatusItem` that is still alive.
/// This function **must** be called on the main thread (the caller must hold a
/// `MainThreadMarker`).
pub(crate) unsafe fn apply_alert_level_raw(
    raw_ptr: *const NSStatusItem,
    level: &AlertLevel,
    mtm: MainThreadMarker,
) {
    let item = unsafe { &*raw_ptr };
    let image = image_for_level(level);
    if let Some(button) = item.button(mtm) {
        button.setImage(Some(&image));
        button.setTitle(&NSString::from_str(""));
    }
}

/// Rebuild and attach a session menu to a raw `NSStatusItem` pointer.
///
/// # Safety
///
/// `raw_ptr` must be a valid pointer to an `NSStatusItem` that is still alive.
/// This function **must** be called on the main thread.
pub(crate) unsafe fn set_menu_raw(raw_ptr: *const NSStatusItem, menu: &NSMenu) {
    let item = unsafe { &*raw_ptr };
    item.setMenu(Some(menu));
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
        let image = image_for_level(level);

        if let Some(button) = self.status_item.button(mtm) {
            button.setImage(Some(&image));
            button.setTitle(&NSString::from_str(""));
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
