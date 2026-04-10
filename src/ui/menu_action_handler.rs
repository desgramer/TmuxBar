use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
use tokio::sync::mpsc;

use crate::models::AppCommand;
use crate::ui::session_menu::{TAG_KILL_SERVER, TAG_NEW_SESSION, TAG_QUIT, TAG_SETTINGS};

// ---------------------------------------------------------------------------
// Ivars
// ---------------------------------------------------------------------------

pub(crate) struct MenuActionHandlerIvars {
    cmd_tx: mpsc::Sender<AppCommand>,
    /// Session names indexed by menu-item tag (0..N-1).
    /// Updated every time the menu is rebuilt.
    session_names: RefCell<Vec<String>>,
}

// ---------------------------------------------------------------------------
// ObjC class definition
// ---------------------------------------------------------------------------

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - MenuActionHandler does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuActionHandlerIvars]
    #[name = "TmuxBarMenuActionHandler"]
    pub(crate) struct MenuActionHandler;

    // SAFETY: NSObjectProtocol has no safety requirements.
    unsafe impl NSObjectProtocol for MenuActionHandler {}

    // Custom ObjC method: called by AppKit when any wired menu item is clicked.
    impl MenuActionHandler {
        // SAFETY: The signature `-(void)menuItemClicked:(id)sender` matches
        // AppKit's target/action convention.
        #[unsafe(method(menuItemClicked:))]
        fn menu_item_clicked(&self, sender: &AnyObject) {
            // Downcast sender to NSMenuItem to read its tag.
            // SAFETY: AppKit guarantees the sender is the NSMenuItem that was clicked.
            let item: &NSMenuItem = unsafe { &*(sender as *const AnyObject as *const NSMenuItem) };
            let tag = item.tag();
            let ivars = self.ivars();

            let cmd = match tag {
                TAG_NEW_SESSION => {
                    let ts = chrono::Utc::now().timestamp() % 100_000;
                    Some(AppCommand::CreateSession {
                        name: format!("s{ts}"),
                    })
                }
                TAG_KILL_SERVER => Some(AppCommand::KillServer),
                TAG_SETTINGS => {
                    tracing::info!("Settings clicked (not yet implemented)");
                    None
                }
                TAG_QUIT => {
                    let _ = ivars.cmd_tx.try_send(AppCommand::Quit);
                    // SAFETY: we are on the main thread (MainThreadOnly).
                    NSApplication::sharedApplication(self.mtm()).terminate(None);
                    return;
                }
                idx if (0..1000).contains(&idx) => {
                    let names = ivars.session_names.borrow();
                    names.get(idx as usize).map(|name: &String| AppCommand::AttachSession {
                        name: name.clone(),
                    })
                }
                _ => None,
            };

            if let Some(command) = cmd {
                if let Err(e) = ivars.cmd_tx.try_send(command) {
                    tracing::error!("Failed to send AppCommand: {e}");
                }
            }
        }
    }
);

// ---------------------------------------------------------------------------
// Public API (Rust side)
// ---------------------------------------------------------------------------

impl MenuActionHandler {
    /// Create a new handler. Must be called on the main thread.
    pub fn new(mtm: MainThreadMarker, cmd_tx: mpsc::Sender<AppCommand>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MenuActionHandlerIvars {
            cmd_tx,
            session_names: RefCell::new(Vec::new()),
        });
        // SAFETY: NSObject's `init` is always safe to call.
        unsafe { msg_send![super(this), init] }
    }

    /// Replace the session-name list. Called each time the menu is rebuilt so
    /// tag→name mapping stays in sync.
    pub fn update_session_names(&self, names: Vec<String>) {
        *self.ivars().session_names.borrow_mut() = names;
    }

    /// The ObjC selector for the click handler.
    pub fn action_sel() -> Sel {
        sel!(menuItemClicked:)
    }
}
