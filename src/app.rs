use std::cell::OnceCell;

use dispatch2::MainThreadBound;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplicationDelegate, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, ns_string};

use crate::launcher::Launcher;

const TOGGLE_HOTKEY_LABEL: &str = "⌥Space";

fn toggle_hotkey() -> HotKey {
    HotKey::new(Some(Modifiers::ALT), Code::Space)
}

#[derive(Default)]
pub struct Ivars {
    launcher: OnceCell<Retained<Launcher>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    hotkeys: OnceCell<GlobalHotKeyManager>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustOverlayAppDelegate"]
    #[ivars = Ivars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let _ = self.ivars().launcher.set(Launcher::new(self.mtm()));
            self.install_status_item();
            self.install_hotkey();
            self.launcher().show();
        }
    }

    impl AppDelegate {
        #[unsafe(method(showLauncher:))]
        fn show_launcher(&self, _sender: Option<&AnyObject>) {
            self.launcher().show();
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn launcher(&self) -> &Launcher {
        self.ivars()
            .launcher
            .get()
            .expect("launcher is created when the app finishes launching")
    }

    /// Adds a menu bar icon, which is the only way to reach the app without the hotkey since
    /// it has no Dock icon.
    fn install_status_item(&self) {
        let mtm = self.mtm();
        let status_item =
            NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = status_item.button(mtm) {
            match NSImage::imageWithSystemSymbolName_accessibilityDescription(
                ns_string!("command.square"),
                Some(ns_string!("Rust Overlay")),
            ) {
                Some(icon) => {
                    icon.setTemplate(true);
                    button.setImage(Some(&icon));
                }
                None => button.setTitle(ns_string!("Overlay")),
            }
        }

        let menu = NSMenu::new(mtm);
        let open_item = unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("Open Launcher"),
                Some(sel!(showLauncher:)),
                ns_string!(""),
            )
        };
        unsafe { open_item.setTarget(Some(self)) };
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        unsafe {
            menu.addItemWithTitle_action_keyEquivalent(
                ns_string!("Quit Rust Overlay"),
                Some(sel!(terminate:)),
                ns_string!("q"),
            )
        };
        status_item.setMenu(Some(&menu));
        let _ = self.ivars().status_item.set(status_item);
    }

    /// Registers the system-wide launcher shortcut. This uses Carbon hotkeys, which don't need
    /// the Accessibility permission.
    fn install_hotkey(&self) {
        let hotkey = toggle_hotkey();
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(err) => {
                eprintln!("rust-overlay: global hotkeys unavailable: {err}");
                return;
            }
        };
        if let Err(err) = manager.register(hotkey) {
            eprintln!("rust-overlay: could not register {TOGGLE_HOTKEY_LABEL}: {err}");
            return;
        }

        let this = MainThreadBound::new(self.retain(), self.mtm());
        let toggle_id = hotkey.id();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.id() != toggle_id || event.state() != HotKeyState::Pressed {
                return;
            }
            // Carbon delivers hotkey events from the main run loop.
            if let Some(mtm) = MainThreadMarker::new() {
                this.get(mtm).launcher().toggle();
            }
        }));
        let _ = self.ivars().hotkeys.set(manager);
    }
}
