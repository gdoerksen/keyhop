//! Spatial ordering of elements before hint labels are assigned.
//!
//! [`crate::HintEngine::generate`] hands out its shortest, most
//! comfortable labels (`a`, `s`, `d`, …) to the *first* elements in the
//! list. Enumeration order from the accessibility tree is roughly
//! top-left reading order, which puts the one-keystroke hints in the
//! corner of the window rather than where the user is usually looking.
//!
//! [`HintOrder`] lets the user pick an anchor point; [`sort_by_proximity`]
//! reorders the elements so those nearest the anchor get the best labels.
//! The sort is O(n log n) over a few hundred rectangles — negligible next
//! to the UIA tree walk that produced them.

use serde::{Deserialize, Serialize};

use crate::model::Element;

/// Which point the shortest hint labels radiate out from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HintOrder {
    /// Center of the scanned area (the foreground window in active-window
    /// scope, the cursor's monitor in active-monitor scope).
    #[default]
    Center,
    /// The mouse cursor position at the moment the picker opens.
    Cursor,
    /// Enumeration order, roughly top-left first (the previous behaviour).
    Reading,
}

/// Stable-sort `elements` by squared distance from each element's center
/// to `anchor`. Ties are broken by reading order (top, then left) so the
/// result is deterministic regardless of enumeration quirks.
pub fn sort_by_proximity(elements: &mut [Element], anchor: (i32, i32)) {
    // i128: two squared i32-range deltas can exceed i64::MAX.
    let (ax, ay) = (anchor.0 as i128, anchor.1 as i128);
    elements.sort_by_cached_key(|el| {
        let (cx, cy) = el.bounds.center();
        let dx = cx as i128 - ax;
        let dy = cy as i128 - ay;
        (dx * dx + dy * dy, el.bounds.y, el.bounds.x)
    });
}

/// Center of the bounding box enclosing every element, or `None` for an
/// empty slice. Used as the fallback anchor when the platform can't
/// report a window / monitor / cursor position.
pub fn union_center(elements: &[Element]) -> Option<(i32, i32)> {
    let first = elements.first()?;
    let (mut l, mut t) = (first.bounds.x as i64, first.bounds.y as i64);
    let (mut r, mut b) = (
        l + first.bounds.width as i64,
        t + first.bounds.height as i64,
    );
    for el in &elements[1..] {
        let bx = el.bounds.x as i64;
        let by = el.bounds.y as i64;
        l = l.min(bx);
        t = t.min(by);
        r = r.max(bx + el.bounds.width as i64);
        b = b.max(by + el.bounds.height as i64);
    }
    Some((((l + r) / 2) as i32, ((t + b) / 2) as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bounds, ElementId, Role};

    fn el(id: u64, x: i32, y: i32) -> Element {
        Element {
            id: ElementId(id),
            role: Role::Button,
            name: None,
            bounds: Bounds {
                x,
                y,
                width: 10,
                height: 10,
            },
        }
    }

    fn ids(v: &[Element]) -> Vec<u64> {
        v.iter().map(|e| e.id.0).collect()
    }

    #[test]
    fn nearest_to_anchor_comes_first() {
        // Reading order: corner (0,0), middle (100,100), far (300,300).
        let mut v = vec![el(1, 0, 0), el(2, 95, 95), el(3, 295, 295)];
        sort_by_proximity(&mut v, (100, 100));
        assert_eq!(ids(&v), vec![2, 1, 3]);
    }

    #[test]
    fn ties_fall_back_to_reading_order() {
        // Four elements equidistant from (100,100), given in scrambled order.
        let mut v = vec![
            el(4, 145, 95), // right
            el(3, 45, 95),  // left
            el(2, 95, 145), // below
            el(1, 95, 45),  // above
        ];
        sort_by_proximity(&mut v, (100, 100));
        assert_eq!(ids(&v), vec![1, 3, 4, 2]);
    }

    #[test]
    fn far_coordinates_do_not_overflow() {
        let mut v = vec![el(1, i32::MAX / 2, i32::MAX / 2), el(2, 0, 0)];
        sort_by_proximity(&mut v, (i32::MIN / 2, i32::MIN / 2));
        assert_eq!(ids(&v), vec![2, 1]);
    }

    #[test]
    fn empty_input_is_fine() {
        let mut v: Vec<Element> = Vec::new();
        sort_by_proximity(&mut v, (0, 0));
        assert!(union_center(&v).is_none());
    }

    #[test]
    fn union_center_spans_all_elements() {
        let v = vec![el(1, 0, 0), el(2, 190, 90)];
        // Box is (0,0)-(200,100).
        assert_eq!(union_center(&v), Some((100, 50)));
    }

    #[test]
    fn hint_order_serializes_as_snake_case() {
        #[derive(Serialize)]
        struct W {
            order: HintOrder,
        }
        let text = toml::to_string(&W {
            order: HintOrder::Cursor,
        })
        .unwrap();
        assert!(text.contains("\"cursor\""), "{text:?}");
    }
}
