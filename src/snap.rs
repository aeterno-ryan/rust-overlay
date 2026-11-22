use objc2_foundation::{NSPoint, NSRect, NSSize};

/// How close (in points) an edge or center must come to a snap line before it snaps.
const SNAP_DISTANCE: f64 = 12.0;
/// Gap kept between the launcher and the screen edges it snaps to.
const EDGE_MARGIN: f64 = 16.0;
/// Where the launcher's top edge sits by default, as a fraction of the visible screen height.
const HOME_TOP: f64 = 0.8;

/// A dashed alignment line shown while dragging, in screen coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide {
    Vertical { x: f64, bottom: f64, top: f64 },
    Horizontal { y: f64, left: f64, right: f64 },
}

impl Guide {
    pub fn offset(self, dx: f64, dy: f64) -> Self {
        match self {
            Guide::Vertical { x, bottom, top } => Guide::Vertical {
                x: x + dx,
                bottom: bottom + dy,
                top: top + dy,
            },
            Guide::Horizontal { y, left, right } => Guide::Horizontal {
                y: y + dy,
                left: left + dx,
                right: right + dx,
            },
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Snapped {
    pub origin: NSPoint,
    pub guides: Vec<Guide>,
}

/// Which part of the panel lines up with a snap line.
#[derive(Clone, Copy)]
enum Anchor {
    Start,
    Center,
    End,
}

impl Anchor {
    fn offset(self, length: f64) -> f64 {
        match self {
            Anchor::Start => 0.0,
            Anchor::Center => length / 2.0,
            Anchor::End => length,
        }
    }
}

/// Keeps `frame` inside `area` (a screen's visible frame), snapping it to the area's center
/// lines, its edges and the launcher's home position when it comes within `SNAP_DISTANCE`.
pub fn snap(frame: NSRect, area: NSRect) -> Snapped {
    let frame = clamp(frame, area);
    let (min, mid, max) = (area.min(), area.mid(), area.max());

    let (x, x_line) = snap_axis(
        frame.origin.x,
        frame.size.width,
        &[
            (mid.x, Anchor::Center),
            (min.x + EDGE_MARGIN, Anchor::Start),
            (max.x - EDGE_MARGIN, Anchor::End),
        ],
    );
    let (y, y_line) = snap_axis(
        frame.origin.y,
        frame.size.height,
        &[
            (mid.y, Anchor::Center),
            (min.y + area.size.height * HOME_TOP, Anchor::End),
            (max.y - EDGE_MARGIN, Anchor::End),
            (min.y + EDGE_MARGIN, Anchor::Start),
        ],
    );

    let vertical = x_line.map(|x| Guide::Vertical {
        x,
        bottom: min.y,
        top: max.y,
    });
    let horizontal = y_line.map(|y| Guide::Horizontal {
        y,
        left: min.x,
        right: max.x,
    });
    Snapped {
        origin: NSPoint::new(x, y),
        guides: vertical.into_iter().chain(horizontal).collect(),
    }
}

/// Returns the snapped position along one axis and the line it snapped to, if any.
fn snap_axis(position: f64, length: f64, lines: &[(f64, Anchor)]) -> (f64, Option<f64>) {
    lines
        .iter()
        .map(|&(line, anchor)| (line - anchor.offset(length), line))
        .filter(|(snapped, _)| (snapped - position).abs() <= SNAP_DISTANCE)
        .min_by(|(a, _), (b, _)| (a - position).abs().total_cmp(&(b - position).abs()))
        .map_or((position, None), |(snapped, line)| (snapped, Some(line)))
}

/// Moves `frame` the minimum distance needed to fit inside `area`.
pub fn clamp(frame: NSRect, area: NSRect) -> NSRect {
    let max = area.max();
    let x = frame
        .origin
        .x
        .min(max.x - frame.size.width)
        .max(area.origin.x);
    let y = frame
        .origin
        .y
        .min(max.y - frame.size.height)
        .max(area.origin.y);
    NSRect::new(NSPoint::new(x, y), frame.size)
}

pub fn contains(rect: NSRect, point: NSPoint) -> bool {
    let (min, max) = (rect.min(), rect.max());
    (min.x..=max.x).contains(&point.x) && (min.y..=max.y).contains(&point.y)
}

/// Where the launcher sits, as fractions of a screen's visible area: its horizontal center and
/// its top edge. Fractions let the position carry over to screens of other sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub center_x: f64,
    pub top: f64,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            center_x: 0.5,
            top: HOME_TOP,
        }
    }
}

impl Placement {
    pub fn of(frame: NSRect, area: NSRect) -> Self {
        Self {
            center_x: (frame.mid().x - area.origin.x) / area.size.width,
            top: (frame.max().y - area.origin.y) / area.size.height,
        }
    }

    pub fn origin(self, size: NSSize, area: NSRect) -> NSPoint {
        let origin = NSPoint::new(
            area.origin.x + self.center_x * area.size.width - size.width / 2.0,
            area.origin.y + self.top * area.size.height - size.height,
        );
        clamp(NSRect::new(origin, size), area).origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: NSRect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1000.0, 800.0));
    const SIZE: NSSize = NSSize::new(200.0, 100.0);

    fn frame_at(x: f64, y: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), SIZE)
    }

    #[test]
    fn snaps_to_horizontal_center_when_close() {
        let snapped = snap(frame_at(408.0, 200.0), AREA);
        assert_eq!(snapped.origin, NSPoint::new(400.0, 200.0));
        assert_eq!(
            snapped.guides,
            [Guide::Vertical {
                x: 500.0,
                bottom: 0.0,
                top: 800.0
            }]
        );
    }

    #[test]
    fn moves_freely_away_from_snap_lines() {
        let snapped = snap(frame_at(430.0, 200.0), AREA);
        assert_eq!(snapped.origin, NSPoint::new(430.0, 200.0));
        assert!(snapped.guides.is_empty());
    }

    #[test]
    fn snaps_top_edge_to_margin_below_screen_top() {
        let snapped = snap(frame_at(100.0, 690.0), AREA);
        assert_eq!(snapped.origin.y, 800.0 - EDGE_MARGIN - SIZE.height);
        assert_eq!(
            snapped.guides,
            [Guide::Horizontal {
                y: 800.0 - EDGE_MARGIN,
                left: 0.0,
                right: 1000.0
            }]
        );
    }

    #[test]
    fn snaps_both_axes_at_once() {
        let snapped = snap(frame_at(395.0, 355.0), AREA);
        assert_eq!(snapped.origin, NSPoint::new(400.0, 350.0));
        assert_eq!(snapped.guides.len(), 2);
    }

    #[test]
    fn stays_inside_the_screen() {
        assert_eq!(
            snap(frame_at(-300.0, 2000.0), AREA).origin,
            NSPoint::new(0.0, 700.0)
        );
    }

    #[test]
    fn default_placement_sits_on_the_home_snap_lines() {
        let origin = Placement::default().origin(SIZE, AREA);
        let snapped = snap(NSRect::new(origin, SIZE), AREA);
        assert_eq!(snapped.origin, origin);
        assert_eq!(snapped.guides.len(), 2);
    }

    #[test]
    fn placement_round_trips_and_scales_to_other_screens() {
        let frame = frame_at(120.0, 300.0);
        assert_eq!(Placement::of(frame, AREA).origin(SIZE, AREA), frame.origin);

        let bigger = NSRect::new(NSPoint::new(1000.0, 0.0), NSSize::new(2000.0, 1600.0));
        let moved = Placement::of(frame, AREA).origin(SIZE, bigger);
        assert_eq!(moved, NSPoint::new(1000.0 + 440.0 - 100.0, 800.0 - 100.0));
    }
}
