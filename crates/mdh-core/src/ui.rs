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
}
