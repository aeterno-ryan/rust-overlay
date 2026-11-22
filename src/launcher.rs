use std::cell::{OnceCell, RefCell};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSControl, NSControlTextEditingDelegate, NSCursor, NSEvent,
    NSEventMask, NSEventModifierFlags, NSEventTrackingRunLoopMode, NSEventType, NSFocusRingType,
    NSFont, NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSPanel, NSScreen, NSStatusWindowLevel, NSTextField,
    NSTextFieldDelegate, NSTextView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowButton, NSWindowCollectionBehavior,
    NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{
    NSDate, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
    NSUserDefaults, ns_string,
};

use crate::catalog::{self, Action, Entry};
use crate::guides::GuideOverlay;
use crate::results::{self, ResultsView};
use crate::snap::{self, Placement, Snapped};

const WIDTH: f64 = 720.0;
const SEARCH_HEIGHT: f64 = 60.0;
const SEARCH_FIELD_HEIGHT: f64 = 30.0;

define_class!(
    /// A floating panel that can take keyboard focus without activating the app, so the app
    /// you were using stays frontmost while you type.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustOverlayLauncherPanel"]
    #[ivars = Weak<Launcher>]
    pub struct LauncherPanel;

    impl LauncherPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        /// Pressing on any empty part of the panel drags it. This is handled here, before the
        /// event reaches a view, because the hidden title bar would otherwise swallow presses
        /// along the top edge.
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            let launcher = self.ivars().load();
            match launcher {
                Some(launcher)
                    if event.r#type() == NSEventType::LeftMouseDown
                        && launcher.is_drag_handle(event) =>
                {
                    launcher.drag();
                }
                _ => unsafe { msg_send![super(self), sendEvent: event] },
            }
        }
    }
);

impl LauncherPanel {
    fn new(mtm: MainThreadMarker, content_rect: NSRect, launcher: &Launcher) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Weak::new(launcher));
        let panel: Retained<Self> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: content_rect,
                styleMask: NSWindowStyleMask::Titled
                    | NSWindowStyleMask::FullSizeContentView
                    | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        // The panel is owned by a `Retained`, so AppKit must not also release it on close.
        unsafe { panel.setReleasedWhenClosed(false) };
        // A hidden title bar keeps the system's rounded corners and shadow.
        panel.setTitlebarAppearsTransparent(true);
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        for kind in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = panel.standardWindowButton(kind) {
                button.setHidden(true);
            }
        }
        panel.setMovable(false);
        panel.setHidesOnDeactivate(false);
        panel.setLevel(NSStatusWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel
    }
}

#[derive(Default)]
pub struct LauncherIvars {
    entries: RefCell<Vec<Entry>>,
    panel: OnceCell<Retained<LauncherPanel>>,
    search: OnceCell<Retained<NSTextField>>,
    results: OnceCell<Retained<ResultsView>>,
    guides: OnceCell<GuideOverlay>,
}

define_class!(
    /// Owns the launcher panel and reacts to typing, arrow keys, Return and Escape.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustOverlayLauncher"]
    #[ivars = LauncherIvars]
    pub struct Launcher;

    unsafe impl NSObjectProtocol for Launcher {}

    unsafe impl NSControlTextEditingDelegate for Launcher {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _notification: &NSNotification) {
            self.refresh_results();
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, _text_view: &NSTextView, command: Sel) -> bool {
            self.handle_command(command)
        }
    }

    unsafe impl NSTextFieldDelegate for Launcher {}

    unsafe impl NSWindowDelegate for Launcher {
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            self.hide();
        }
    }
);

impl Launcher {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(LauncherIvars::default());
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.build_panel();
        this
    }

    fn build_panel(&self) {
        let mtm = self.mtm();
        let content_rect = NSRect::new(
            NSPoint::ZERO,
            NSSize::new(WIDTH, SEARCH_HEIGHT + results::HEIGHT),
        );
        let panel = LauncherPanel::new(mtm, content_rect, self);
        panel.setDelegate(Some(ProtocolObject::from_ref(self)));

        let background =
            NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), content_rect);
        background.setMaterial(NSVisualEffectMaterial::Popover);
        background.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        // The app is never active, so the material must not fall back to its inactive look.
        background.setState(NSVisualEffectState::Active);
        panel.setContentView(Some(&background));

        let search = NSTextField::initWithFrame(
            NSTextField::alloc(mtm),
            NSRect::new(
                NSPoint::new(
                    20.0,
                    results::HEIGHT + (SEARCH_HEIGHT - SEARCH_FIELD_HEIGHT) / 2.0,
                ),
                NSSize::new(WIDTH - 40.0, SEARCH_FIELD_HEIGHT),
            ),
        );
        search.setBezeled(false);
        search.setBordered(false);
        search.setDrawsBackground(false);
        search.setFocusRingType(NSFocusRingType::None);
        search.setFont(Some(&NSFont::systemFontOfSize(22.0)));
        search.setPlaceholderString(Some(ns_string!("Search apps and commands…")));
        unsafe { search.setDelegate(Some(ProtocolObject::from_ref(self))) };
        background.addSubview(&search);

        let results = ResultsView::new(
            mtm,
            NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, results::HEIGHT)),
            self,
        );
        background.addSubview(&results);

        let ivars = self.ivars();
        let _ = ivars.panel.set(panel);
        let _ = ivars.search.set(search);
        let _ = ivars.results.set(results);
        let _ = ivars.guides.set(GuideOverlay::new(mtm));
    }

    fn panel(&self) -> &LauncherPanel {
        self.ivars()
            .panel
            .get()
            .expect("panel is built in Launcher::new")
    }

    fn search(&self) -> &NSTextField {
        self.ivars()
            .search
            .get()
            .expect("search field is built in Launcher::new")
    }

    fn results(&self) -> &ResultsView {
        self.ivars()
            .results
            .get()
            .expect("results view is built in Launcher::new")
    }

    fn guides(&self) -> &GuideOverlay {
        self.ivars()
            .guides
            .get()
            .expect("guides are built in Launcher::new")
    }

    pub fn toggle(&self) {
        if self.panel().isVisible() {
            self.hide();
        } else {
            self.show();
        }
    }

    pub fn show(&self) {
        *self.ivars().entries.borrow_mut() = catalog::load();
        self.refresh_results();

        let panel = self.panel();
        if let Some(screen) = NSScreen::mainScreen(self.mtm()) {
            panel.setFrameOrigin(
                saved_placement().origin(panel.frame().size, screen.visibleFrame()),
            );
        }
        panel.makeKeyAndOrderFront(None);
        // Focusing the field selects any previous query, so typing replaces it.
        panel.makeFirstResponder(Some(self.search()));
    }

    pub fn hide(&self) {
        let panel = self.panel();
        if panel.isVisible() {
            panel.orderOut(None);
        }
    }

    /// Whether a press at `event` should drag the panel: anywhere except the search field and
    /// the result rows.
    fn is_drag_handle(&self, event: &NSEvent) -> bool {
        let point = event.locationInWindow();
        let Some(hit) = self
            .panel()
            .contentView()
            .and_then(|content| content.hitTest(point))
        else {
            return false;
        };
        !hit.isDescendantOf(self.search()) && !self.results().is_row_at(point)
    }

    /// Moves the panel with the mouse until the button is released, snapping it to screen
    /// guides along the way. Holding ⌘ places it freely.
    fn drag(&self) {
        let mtm = self.mtm();
        let panel = self.panel();
        let start_mouse = NSEvent::mouseLocation();
        let start = panel.frame();
        let mut screen = None;
        let mut engaged = Vec::new();

        let cursor = NSCursor::closedHandCursor();
        cursor.push();
        while let Some(event) = panel.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp,
            Some(&NSDate::distantFuture()),
            unsafe { NSEventTrackingRunLoopMode },
            true,
        ) {
            if event.r#type() == NSEventType::LeftMouseUp {
                break;
            }
            let mouse = NSEvent::mouseLocation();
            let Some(current) = NSScreen::screens(mtm)
                .iter()
                .find(|screen| snap::contains(screen.frame(), mouse))
            else {
                continue;
            };
            let proposed = NSRect::new(
                NSPoint::new(
                    start.origin.x + mouse.x - start_mouse.x,
                    start.origin.y + mouse.y - start_mouse.y,
                ),
                start.size,
            );
            let area = current.visibleFrame();
            let snapped = if event
                .modifierFlags()
                .contains(NSEventModifierFlags::Command)
            {
                Snapped {
                    origin: snap::clamp(proposed, area).origin,
                    guides: Vec::new(),
                }
            } else {
                snap::snap(proposed, area)
            };

            if snapped.guides.iter().any(|guide| !engaged.contains(guide)) {
                NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
                    NSHapticFeedbackPattern::Alignment,
                    NSHapticFeedbackPerformanceTime::Now,
                );
            }
            panel.setFrameOrigin(snapped.origin);
            self.guides().show(&snapped.guides, &current, panel);
            engaged = snapped.guides;
            screen = Some(current);
        }
        cursor.pop();
        self.guides().hide();

        if let Some(screen) = screen {
            save_placement(Placement::of(panel.frame(), screen.visibleFrame()));
        }
    }

    /// Handles the search field's navigation keys, returning whether the key was consumed.
    fn handle_command(&self, command: Sel) -> bool {
        if command == sel!(moveUp:) {
            self.results().move_selection(-1);
        } else if command == sel!(moveDown:) {
            self.results().move_selection(1);
        } else if command == sel!(insertNewline:) {
            self.activate_selected();
        } else if command == sel!(cancelOperation:) {
            self.cancel();
        } else {
            return false;
        }
        true
    }

    /// Escape clears the query first, and closes the launcher once it's empty.
    fn cancel(&self) {
        let search = self.search();
        if search.stringValue().length() > 0 {
            search.setStringValue(ns_string!(""));
            self.refresh_results();
        } else {
            self.hide();
        }
    }

    fn refresh_results(&self) {
        let query = self.search().stringValue().to_string();
        let rows = catalog::search(&self.ivars().entries.borrow(), &query);
        self.results().set_rows(rows);
    }

    pub fn activate_selected(&self) {
        let Some(entry) = self.results().selected() else {
            return;
        };
        self.hide();
        self.search().setStringValue(ns_string!(""));

        let workspace = NSWorkspace::sharedWorkspace();
        match entry.action {
            Action::Open(path) => {
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                workspace.openURL(&url);
            }
            Action::OpenUrl(url) => {
                if let Some(url) = NSURL::URLWithString(&NSString::from_str(&url)) {
                    workspace.openURL(&url);
                }
            }
            Action::Quit => NSApplication::sharedApplication(self.mtm()).terminate(None),
        }
    }
}

fn placement_keys() -> (&'static NSString, &'static NSString) {
    (ns_string!("LauncherCenterX"), ns_string!("LauncherTop"))
}

fn saved_placement() -> Placement {
    let defaults = NSUserDefaults::standardUserDefaults();
    let (center_x, top) = placement_keys();
    if defaults.objectForKey(center_x).is_none() || defaults.objectForKey(top).is_none() {
        return Placement::default();
    }
    Placement {
        center_x: defaults.doubleForKey(center_x),
        top: defaults.doubleForKey(top),
    }
}

fn save_placement(placement: Placement) {
    let defaults = NSUserDefaults::standardUserDefaults();
    let (center_x, top) = placement_keys();
    defaults.setDouble_forKey(placement.center_x, center_x);
    defaults.setDouble_forKey(placement.top, top);
}
