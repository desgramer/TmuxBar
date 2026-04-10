use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSAlert, NSMenuItem, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSRect, NSPoint, NSSize, NSString};
use tokio::sync::mpsc;

use crate::i18n::{self, Language};
use crate::models::AppCommand;
use crate::ui::session_menu::{TAG_KILL_SERVER, TAG_KILL_SESSION_BASE, TAG_NEW_SESSION, TAG_QUIT, TAG_RENAME_SESSION_BASE, TAG_SETTINGS};

// ---------------------------------------------------------------------------
// Ivars
// ---------------------------------------------------------------------------

pub(crate) struct MenuActionHandlerIvars {
    cmd_tx: mpsc::Sender<AppCommand>,
    /// Session names indexed by menu-item tag (0..N-1).
    /// Updated every time the menu is rebuilt.
    session_names: RefCell<Vec<String>>,
    language: RefCell<Language>,
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
                    let placeholder = format!("s{ts}");

                    let lang = ivars.language.borrow();
                    let alert = NSAlert::new(self.mtm());
                    alert.setMessageText(&NSString::from_str(i18n::alert_new_session_title(&lang)));
                    alert.setInformativeText(&NSString::from_str(i18n::alert_new_session_message(&lang)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_create(&lang)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_cancel(&lang)));

                    let text_field = NSTextField::initWithFrame(
                        NSTextField::alloc(self.mtm()),
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 24.0)),
                    );
                    text_field.setPlaceholderString(Some(&NSString::from_str(&placeholder)));
                    let view_ref: &NSView = &text_field;
                    alert.setAccessoryView(Some(view_ref));

                    let window = alert.window();
                    let responder_ref: &objc2_app_kit::NSResponder = &text_field;
                    window.makeFirstResponder(Some(responder_ref));

                    let response = alert.runModal();
                    if response == NSAlertFirstButtonReturn {
                        let name_ns = text_field.stringValue();
                        let name = name_ns.to_string();
                        let name = if name.trim().is_empty() {
                            placeholder
                        } else {
                            name.trim().to_string()
                        };
                        Some(AppCommand::CreateSession { name })
                    } else {
                        None
                    }
                }
                TAG_KILL_SERVER => Some(AppCommand::KillServer),
                TAG_SETTINGS => Some(AppCommand::OpenSettings),
                TAG_QUIT => {
                    let _ = ivars.cmd_tx.try_send(AppCommand::Quit);
                    // SAFETY: we are on the main thread (MainThreadOnly).
                    NSApplication::sharedApplication(self.mtm()).terminate(None);
                    return;
                }
                // Attach: tags 0..999
                idx if (0..1000).contains(&idx) => {
                    let names = ivars.session_names.borrow();
                    names.get(idx as usize).map(|name: &String| AppCommand::AttachSession {
                        name: name.clone(),
                    })
                }
                // Rename: tags 3000..3999
                idx if (TAG_RENAME_SESSION_BASE..TAG_RENAME_SESSION_BASE + 1000).contains(&idx) => {
                    let session_idx = (idx - TAG_RENAME_SESSION_BASE) as usize;
                    let names = ivars.session_names.borrow();
                    let old_name = match names.get(session_idx) {
                        Some(n) => n.clone(),
                        None => return,
                    };
                    drop(names);

                    let lang = ivars.language.borrow();
                    let alert = NSAlert::new(self.mtm());
                    alert.setMessageText(&NSString::from_str(i18n::alert_rename_title(&lang)));
                    alert.setInformativeText(&NSString::from_str(&i18n::alert_rename_message(&lang, &old_name)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_rename_confirm(&lang)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_cancel(&lang)));

                    let text_field = NSTextField::initWithFrame(
                        NSTextField::alloc(self.mtm()),
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 24.0)),
                    );
                    text_field.setStringValue(&NSString::from_str(&old_name));
                    let view_ref: &NSView = &text_field;
                    alert.setAccessoryView(Some(view_ref));

                    let window = alert.window();
                    let responder_ref: &objc2_app_kit::NSResponder = &text_field;
                    window.makeFirstResponder(Some(responder_ref));

                    let response = alert.runModal();
                    if response == NSAlertFirstButtonReturn {
                        let new_name_ns = text_field.stringValue();
                        let new_name = new_name_ns.to_string().trim().to_string();
                        if !new_name.is_empty() && new_name != old_name {
                            Some(AppCommand::RenameSession { old_name, new_name })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                // Kill: tags 2000..2999
                idx if (TAG_KILL_SESSION_BASE..TAG_KILL_SESSION_BASE + 1000).contains(&idx) => {
                    let session_idx = (idx - TAG_KILL_SESSION_BASE) as usize;
                    let names = ivars.session_names.borrow();
                    let name = match names.get(session_idx) {
                        Some(n) => n.clone(),
                        None => return,
                    };
                    drop(names);

                    let lang = ivars.language.borrow();
                    let alert = NSAlert::new(self.mtm());
                    alert.setAlertStyle(NSAlertStyle::Warning);
                    alert.setMessageText(&NSString::from_str(i18n::alert_kill_title(&lang)));
                    alert.setInformativeText(&NSString::from_str(&i18n::alert_kill_confirm(&lang, &name)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_confirm_kill(&lang)));
                    alert.addButtonWithTitle(&NSString::from_str(i18n::alert_cancel(&lang)));

                    let response = alert.runModal();
                    if response == NSAlertFirstButtonReturn {
                        Some(AppCommand::KillSession { name })
                    } else {
                        None
                    }
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
    pub fn new(mtm: MainThreadMarker, cmd_tx: mpsc::Sender<AppCommand>, lang: Language) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MenuActionHandlerIvars {
            cmd_tx,
            session_names: RefCell::new(Vec::new()),
            language: RefCell::new(lang),
        });
        // SAFETY: NSObject's `init` is always safe to call.
        unsafe { msg_send![super(this), init] }
    }

    /// Replace the session-name list. Called each time the menu is rebuilt so
    /// tag→name mapping stays in sync.
    pub fn update_session_names(&self, names: Vec<String>) {
        *self.ivars().session_names.borrow_mut() = names;
    }

    /// Update the display language used in alert dialogs.
    pub fn update_language(&self, lang: Language) {
        *self.ivars().language.borrow_mut() = lang;
    }

    /// The ObjC selector for the click handler.
    pub fn action_sel() -> Sel {
        sel!(menuItemClicked:)
    }
}
