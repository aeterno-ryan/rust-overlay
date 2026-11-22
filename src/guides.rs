use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSPanel, NSScreen, NSStatusWindowLevel, NSView,
    NSWindow, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect};

use crate::snap::Guide;

define_class!(
    /// Draws snap guides, stored in this view's coordinates.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustOverlayGuideView"]
    #[ivars = RefCell<Vec<Guide>>]
    struct GuideView;

    impl GuideView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let path = NSBezierPath::bezierPath();
            path.setLineWidth(2.0);
            let dash = [8.0, 6.0];
            unsafe { path.setLineDash_count_phase(dash.as_ptr(), dash.len() as isize, 0.0) };
            for guide in self.ivars().borrow().iter() {
                let (from, to) = match *guide {
                    Guide::Vertical { x, bottom, top } => (NSPoint::new(x, bottom), NSPoint::new(x, top)),
                    Guide::Horizontal { y, left, right } => (NSPoint::new(left, y), NSPoint::new(right, y)),
                };
                path.moveToPoint(from);
                path.lineToPoint(to);
            }
            NSColor::controlAccentColor().setStroke();
            path.stroke();
        }
    }
);

/// A transparent, click-through window that shows dashed snap guides across a screen.
pub struct GuideOverlay {
    window: Retained<NSPanel>,
    view: Retained<GuideView>,
}

impl GuideOverlay {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::ZERO,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        // The window is owned by a `Retained`, so AppKit must not also release it on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        window.setHidesOnDeactivate(false);
        window.setLevel(NSStatusWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );

        let view = GuideView::alloc(mtm).set_ivars(RefCell::default());
        let view: Retained<GuideView> = unsafe { msg_send![super(view), init] };
        window.setContentView(Some(&view));
        Self { window, view }
    }

    /// Shows `guides` (in screen coordinates) across `screen`, just below `window`.
    pub fn show(&self, guides: &[Guide], screen: &NSScreen, window: &NSWindow) {
        if guides.is_empty() {
            self.hide();
            return;
        }
        let frame = screen.frame();
        self.window.setFrame_display(frame, false);
        *self.view.ivars().borrow_mut() = guides
            .iter()
            .map(|guide| guide.offset(-frame.origin.x, -frame.origin.y))
            .collect();
        self.view.setNeedsDisplay(true);
        self.window
            .orderWindow_relativeTo(NSWindowOrderingMode::Below, window.windowNumber());
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }
}
