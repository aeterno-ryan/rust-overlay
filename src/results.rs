use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSCompositingOperation, NSEvent, NSFont, NSFontAttributeName,
    NSFontWeightSemibold, NSForegroundColorAttributeName, NSImage, NSImageSymbolConfiguration,
    NSLineBreakMode, NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStringDrawing,
    NSView, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSAttributedStringKey, NSDictionary, NSPoint, NSRect, NSSize, NSString, ns_string,
};

use crate::catalog::{Entry, Icon, Tint};
use crate::launcher::Launcher;

const ROW_HEIGHT: f64 = 44.0;
const VISIBLE_ROWS: usize = 8;
const LIST_PADDING: f64 = 6.0;
const ROW_INSET: f64 = 8.0;
const ICON_SIZE: f64 = 28.0;
const FOOTER_HEIGHT: f64 = 36.0;

/// Height of the results list including its footer.
pub const HEIGHT: f64 = 2.0 * LIST_PADDING + ROW_HEIGHT * VISIBLE_ROWS as f64 + FOOTER_HEIGHT;

pub struct ResultsIvars {
    launcher: Weak<Launcher>,
    rows: RefCell<Vec<Entry>>,
    selected: Cell<usize>,
    first_visible: Cell<usize>,
    scroll_remainder: Cell<f64>,
    icons: RefCell<HashMap<Icon, Retained<NSImage>>>,
}

define_class!(
    /// The list of search results below the search field, drawn by hand so each row can show
    /// an icon, a title and the entry's kind.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustOverlayResultsView"]
    #[ivars = ResultsIvars]
    pub struct ResultsView;

    impl ResultsView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            self.draw();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let Some(index) = self.row_at(point) else {
                return;
            };
            self.ivars().selected.set(index);
            self.setNeedsDisplay(true);
            if let Some(launcher) = self.ivars().launcher.load() {
                launcher.activate_selected();
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let mut delta = event.scrollingDeltaY();
            if !event.hasPreciseScrollingDeltas() {
                delta *= ROW_HEIGHT;
            }
            let total = self.ivars().scroll_remainder.get() + delta;
            let rows = (total / ROW_HEIGHT).trunc();
            self.ivars().scroll_remainder.set(total - rows * ROW_HEIGHT);
            if rows != 0.0 {
                self.scroll_by(-(rows as isize));
            }
        }
    }
);

impl ResultsView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, launcher: &Launcher) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ResultsIvars {
            launcher: Weak::new(launcher),
            rows: RefCell::default(),
            selected: Cell::new(0),
            first_visible: Cell::new(0),
            scroll_remainder: Cell::new(0.0),
            icons: RefCell::default(),
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    pub fn set_rows(&self, rows: Vec<Entry>) {
        *self.ivars().rows.borrow_mut() = rows;
        self.ivars().selected.set(0);
        self.ivars().first_visible.set(0);
        self.setNeedsDisplay(true);
    }

    pub fn selected(&self) -> Option<Entry> {
        self.ivars()
            .rows
            .borrow()
            .get(self.ivars().selected.get())
            .cloned()
    }

    /// Moves the selection by `delta` rows, scrolling to keep it visible.
    pub fn move_selection(&self, delta: isize) {
        let ivars = self.ivars();
        let count = ivars.rows.borrow().len();
        if count == 0 {
            return;
        }
        let selected = ivars
            .selected
            .get()
            .saturating_add_signed(delta)
            .min(count - 1);
        ivars.selected.set(selected);

        let first = ivars.first_visible.get();
        if selected < first {
            ivars.first_visible.set(selected);
        } else if selected >= first + VISIBLE_ROWS {
            ivars.first_visible.set(selected + 1 - VISIBLE_ROWS);
        }
        self.setNeedsDisplay(true);
    }

    fn scroll_by(&self, rows: isize) {
        let ivars = self.ivars();
        let max_first = ivars.rows.borrow().len().saturating_sub(VISIBLE_ROWS);
        let first = ivars
            .first_visible
            .get()
            .saturating_add_signed(rows)
            .min(max_first);
        ivars.first_visible.set(first);
        self.setNeedsDisplay(true);
    }

    pub fn is_row_at(&self, window_point: NSPoint) -> bool {
        self.row_at(self.convertPoint_fromView(window_point, None))
            .is_some()
    }

    fn row_at(&self, point: NSPoint) -> Option<usize> {
        let offset = point.y - LIST_PADDING;
        if offset < 0.0 {
            return None;
        }
        let visible_index = (offset / ROW_HEIGHT) as usize;
        let index = self.ivars().first_visible.get() + visible_index;
        (visible_index < VISIBLE_ROWS && index < self.ivars().rows.borrow().len()).then_some(index)
    }

    fn row_rect(&self, visible_index: usize) -> NSRect {
        NSRect::new(
            NSPoint::new(ROW_INSET, LIST_PADDING + ROW_HEIGHT * visible_index as f64),
            NSSize::new(self.bounds().size.width - 2.0 * ROW_INSET, ROW_HEIGHT),
        )
    }

    fn draw(&self) {
        let ivars = self.ivars();
        let size = self.bounds().size;

        NSColor::separatorColor().setFill();
        NSBezierPath::fillRect(NSRect::new(NSPoint::ZERO, NSSize::new(size.width, 1.0)));
        let footer_top = size.height - FOOTER_HEIGHT;
        NSBezierPath::fillRect(NSRect::new(
            NSPoint::new(0.0, footer_top),
            NSSize::new(size.width, 1.0),
        ));

        let title_attrs = text_attributes(&NSFont::systemFontOfSize(14.0), &NSColor::labelColor());
        let detail_attrs = text_attributes(
            &NSFont::systemFontOfSize(12.0),
            &NSColor::secondaryLabelColor(),
        );

        let first = ivars.first_visible.get();
        let rows = ivars.rows.borrow();
        for (index, entry) in rows.iter().enumerate().skip(first).take(VISIBLE_ROWS) {
            let row = self.row_rect(index - first);
            if index == ivars.selected.get() {
                NSColor::labelColor().colorWithAlphaComponent(0.1).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(row, 8.0, 8.0).fill();
            }

            let icon_rect = NSRect::new(
                NSPoint::new(
                    row.origin.x + 10.0,
                    row.origin.y + (ROW_HEIGHT - ICON_SIZE) / 2.0,
                ),
                NSSize::new(ICON_SIZE, ICON_SIZE),
            );
            self.draw_icon(&entry.icon, icon_rect);

            let kind = NSString::from_str(entry.kind);
            let kind_size = unsafe { kind.sizeWithAttributes(Some(&detail_attrs)) };
            let kind_x = row.origin.x + row.size.width - 12.0 - kind_size.width;
            unsafe {
                kind.drawAtPoint_withAttributes(
                    NSPoint::new(kind_x, row.origin.y + (ROW_HEIGHT - kind_size.height) / 2.0),
                    Some(&detail_attrs),
                );
            }

            let title = NSString::from_str(&entry.title);
            let title_height = unsafe { title.sizeWithAttributes(Some(&title_attrs)) }.height;
            let title_x = icon_rect.origin.x + ICON_SIZE + 12.0;
            let title_rect = NSRect::new(
                NSPoint::new(title_x, row.origin.y + (ROW_HEIGHT - title_height) / 2.0),
                NSSize::new(kind_x - 16.0 - title_x, title_height),
            );
            unsafe { title.drawInRect_withAttributes(title_rect, Some(&title_attrs)) };
        }

        let hints = ns_string!("Open ↵     Close esc");
        let hints_size = unsafe { hints.sizeWithAttributes(Some(&detail_attrs)) };
        let footer_text_y = footer_top + (FOOTER_HEIGHT - hints_size.height) / 2.0;
        unsafe {
            ns_string!("Rust Overlay")
                .drawAtPoint_withAttributes(NSPoint::new(16.0, footer_text_y), Some(&detail_attrs));
            hints.drawAtPoint_withAttributes(
                NSPoint::new(size.width - 16.0 - hints_size.width, footer_text_y),
                Some(&detail_attrs),
            );
        }
    }

    fn draw_icon(&self, icon: &Icon, rect: NSRect) {
        let image = self
            .ivars()
            .icons
            .borrow_mut()
            .entry(icon.clone())
            .or_insert_with(|| load_icon(icon))
            .clone();

        let target = match icon {
            Icon::File(_) => rect,
            Icon::Symbol { tint, .. } => {
                tint_color(*tint).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 7.0, 7.0).fill();
                aspect_fit(image.size(), rect, 6.0)
            }
        };
        unsafe {
            image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                target,
                NSRect::ZERO,
                NSCompositingOperation::SourceOver,
                1.0,
                true,
                None,
            );
        }
    }
}

fn load_icon(icon: &Icon) -> Retained<NSImage> {
    match icon {
        Icon::File(path) => {
            NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()))
        }
        Icon::Symbol { name, .. } => {
            let config =
                NSImageSymbolConfiguration::configurationWithPointSize_weight(14.0, unsafe {
                    NSFontWeightSemibold
                })
                .configurationByApplyingConfiguration(
                    &NSImageSymbolConfiguration::configurationWithPaletteColors(
                        &NSArray::from_retained_slice(&[NSColor::whiteColor()]),
                    ),
                );
            NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(name),
                None,
            )
            .and_then(|image| image.imageWithSymbolConfiguration(&config))
            .unwrap_or_default()
        }
    }
}

fn tint_color(tint: Tint) -> Retained<NSColor> {
    match tint {
        Tint::Blue => NSColor::systemBlueColor(),
        Tint::Red => NSColor::systemRedColor(),
    }
}

/// The largest rect with `size`'s aspect ratio that fits centered inside `bounds` inset by `inset`.
fn aspect_fit(size: NSSize, bounds: NSRect, inset: f64) -> NSRect {
    let available = NSSize::new(
        bounds.size.width - 2.0 * inset,
        bounds.size.height - 2.0 * inset,
    );
    if size.width <= 0.0 || size.height <= 0.0 {
        return bounds;
    }
    let scale = (available.width / size.width).min(available.height / size.height);
    let fitted = NSSize::new(size.width * scale, size.height * scale);
    NSRect::new(
        NSPoint::new(
            bounds.origin.x + (bounds.size.width - fitted.width) / 2.0,
            bounds.origin.y + (bounds.size.height - fitted.height) / 2.0,
        ),
        fitted,
    )
}

fn text_attributes(
    font: &NSFont,
    color: &NSColor,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    let keys = unsafe {
        [
            NSFontAttributeName,
            NSForegroundColorAttributeName,
            NSParagraphStyleAttributeName,
        ]
    };
    let values: [&AnyObject; 3] = [font, color, &paragraph];
    NSDictionary::from_slices(&keys, &values)
}
