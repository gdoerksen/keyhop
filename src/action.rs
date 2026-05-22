//! Actions that can be performed against an [`crate::Element`].

/// Things keyhop can do to a target element. Backends choose the best native
/// mechanism (UI Automation invoke pattern, synthesized input, etc.) and may
/// return an error for actions a given element does not support.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// Trigger the element's primary action (button press, link follow, ...).
    Invoke,
    /// Move keyboard focus to the element without invoking it.
    Focus,
    /// Synthesize a left mouse click at the element's center.
    Click,
    /// Synthesize a right mouse click at the element's center.
    ///
    /// Exposed via the overlay's Shift modifier on a hint letter so the
    /// user can pop the element's context menu instead of invoking it.
    /// Unlike [`Action::Invoke`], this path never goes through a UIA
    /// pattern — context menus aren't expressible as UIA actions, so
    /// synthesized input is the only mechanism that works.
    RightClick,
    /// Synthesize a double left-click at the element's center.
    ///
    /// Exposed via the overlay's Ctrl modifier on a hint letter. Used
    /// for controls that genuinely need a second click (file-list rows,
    /// some custom controls that distinguish single from double click).
    /// Both clicks land within the system's `GetDoubleClickTime` window
    /// by issuing them in a single `SendInput` batch.
    DoubleClick,
    /// Type the given string into the element.
    Type(String),
    /// Scroll the element's container by the given delta in physical pixels.
    Scroll {
        /// Horizontal delta in pixels.
        dx: i32,
        /// Vertical delta in pixels.
        dy: i32,
    },
}
