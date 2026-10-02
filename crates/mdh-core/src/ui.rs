//! Backend-agnostic UI hierarchy model. Each driver converts its native format into [`RawNode`]s.

use serde::{Deserialize, Serialize};

/// Screen-space rectangle in device pixels; `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(&self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(&self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    pub fn area(&self) -> i64 {
        i64::from(self.width()) * i64::from(self.height())
    }

    pub fn is_empty(&self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    pub fn center(&self) -> (i32, i32) {
        (self.left + self.width() / 2, self.top + self.height() / 2)
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let r = Rect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        );
        (!r.is_empty()).then_some(r)
    }

    /// The parts of `self` not covered by `other` (up to four rectangles).
    pub fn subtract(&self, other: &Rect) -> Vec<Rect> {
        let Some(cut) = self.intersect(other) else {
            return vec![*self];
        };
        [
            Rect::new(self.left, self.top, self.right, cut.top), // above
            Rect::new(self.left, cut.bottom, self.right, self.bottom), // below
            Rect::new(self.left, cut.top, cut.left, cut.bottom), // left
            Rect::new(cut.right, cut.top, self.right, cut.bottom), // right
        ]
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect()
    }

    /// The largest part of `self` not covered by any of `covers`, if any is left.
    pub fn largest_visible_part(&self, covers: &[Rect]) -> Option<Rect> {
        let mut parts = vec![*self];
        for cover in covers {
            parts = parts.iter().flat_map(|p| p.subtract(cover)).collect();
        }
        parts.into_iter().max_by_key(Rect::area)
    }

    pub fn union(&self, other: &Rect) -> Rect {
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeFlags {
    pub clickable: bool,
    pub long_clickable: bool,
    pub checkable: bool,
    pub checked: bool,
    pub enabled: bool,
    pub focusable: bool,
    pub focused: bool,
    pub scrollable: bool,
    pub selected: bool,
    pub password: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RawNode {
    /// Native class name, e.g. `android.widget.Button`.
    pub class: String,
    pub package: Option<String>,
    pub resource_id: Option<String>,
    pub text: Option<String>,
    /// Accessibility description (`content-desc` on Android, `accessibilityLabel` on iOS).
    pub desc: Option<String>,
    /// Placeholder shown in empty inputs.
    pub hint: Option<String>,
    /// Role announced by the toolkit when the class is generic, e.g. `Switch` for a Compose row
    /// with `Role.Switch` (Android's `AccessibilityNodeInfo.roleDescription`).
    pub role_description: Option<String>,
    pub bounds: Rect,
    pub flags: NodeFlags,
    pub children: Vec<RawNode>,
}

/// Where a hierarchy came from; the backends differ in speed and capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeSource {
    /// The on-device helper's warm UiAutomation connection.
    Helper,
    /// `uiautomator dump`, ~2 s per call; the fallback when the helper can't run.
    Uiautomator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawTree {
    /// One root per window.
    pub roots: Vec<RawNode>,
    pub source: TreeSource,
    /// On-screen windows, top-most first. Empty when the backend can't list them.
    #[serde(default)]
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    Application,
    InputMethod,
    System,
    AccessibilityOverlay,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub kind: WindowKind,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub focused: bool,
    pub title: Option<String>,
    pub package: Option<String>,
    pub bounds: Rect,
}

/// What is in front of the user, beyond the UI tree.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScreenInfo {
    /// Focused activity as `package/.Activity`, as reported by the window manager.
    pub activity: Option<String>,
    pub size: Rect,
    pub keyboard: bool,
    /// Package of a window covering the app that belongs to someone else, typically a system
    /// dialog (permission prompt, ANR or crash dialog). Agents must deal with it first.
    pub overlay: Option<String>,
    /// Screen areas covered by system windows drawn over the app (status and navigation bars,
    /// the keyboard). Taps avoid them; apps drawing edge-to-edge can have content underneath.
    #[serde(default)]
    pub obstructions: Vec<Rect>,
}

impl ScreenInfo {
    pub fn new(activity: Option<String>, size: Rect, windows: &[WindowInfo]) -> Self {
        let app_package = activity
            .as_deref()
            .and_then(|a| a.split_once('/'))
            .map(|(package, _)| package);
        let overlay = windows
            .iter()
            .filter(|w| w.active || w.focused)
            .filter(|w| w.kind != WindowKind::InputMethod)
            .find_map(|w| w.package.as_deref().filter(|p| Some(*p) != app_package))
            .map(str::to_owned);
        let obstructions = windows
            .iter()
            .filter(|w| matches!(w.kind, WindowKind::System | WindowKind::InputMethod))
            .filter(|w| !w.active && !w.focused)
            .map(|w| w.bounds)
            .filter(|b| !b.is_empty())
            .collect();
        // A dialog's tree only spans the dialog; the windows span the display.
        let size = windows
            .iter()
            .map(|w| w.bounds)
            .reduce(|a, b| a.union(&b))
            .unwrap_or(size);
        Self {
            activity,
            size,
            keyboard: windows.iter().any(|w| w.kind == WindowKind::InputMethod),
            overlay,
            obstructions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(kind: WindowKind, package: &str, active: bool) -> WindowInfo {
        WindowInfo {
            kind,
            active,
            focused: active,
            title: None,
            package: Some(package.into()),
            bounds: Rect::default(),
        }
    }

    #[test]
    fn visible_part_avoids_bars() {
        let button = Rect::new(48, 48, 1296, 192);
        let status_bar = Rect::new(0, 0, 1344, 159);
        assert_eq!(
            button.largest_visible_part(&[status_bar]),
            Some(Rect::new(48, 159, 1296, 192))
        );
        assert_eq!(
            button.largest_visible_part(&[Rect::new(0, 0, 2000, 2000)]),
            None
        );
        assert_eq!(button.largest_visible_part(&[]), Some(button));
    }

    #[test]
    fn detects_keyboard_and_foreign_overlay() {
        let activity = Some("com.example/.MainActivity".to_owned());
        let plain = ScreenInfo::new(
            activity.clone(),
            Rect::default(),
            &[window(WindowKind::Application, "com.example", true)],
        );
        assert!(!plain.keyboard);
        assert_eq!(plain.overlay, None);

        let prompt = ScreenInfo::new(
            activity,
            Rect::default(),
            &[
                window(
                    WindowKind::Application,
                    "com.android.permissioncontroller",
                    true,
                ),
                window(
                    WindowKind::InputMethod,
                    "com.google.android.inputmethod.latin",
                    false,
                ),
                window(WindowKind::Application, "com.example", false),
            ],
        );
        assert!(prompt.keyboard);
        assert_eq!(
            prompt.overlay.as_deref(),
            Some("com.android.permissioncontroller")
        );
    }
}
