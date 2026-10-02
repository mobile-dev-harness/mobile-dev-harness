use mdh_core::ui::ScreenInfo;
use mdh_observe::{TreeDiff, UiTree};
use serde::{Deserialize, Serialize};

use crate::target::Target;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum Action {
    Tap {
        target: Target,
    },
    LongPress {
        target: Target,
        duration_ms: u32,
    },
    /// Replaces (or with `append`, extends) the focused field's text, after tapping `into` if given.
    Type {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        into: Option<Target>,
        #[serde(default)]
        append: bool,
        /// Press ENTER afterwards.
        #[serde(default)]
        enter: bool,
    },
    Swipe {
        from: (i32, i32),
        to: (i32, i32),
        duration_ms: u32,
    },
    /// Scrolls to reveal content in `direction`, optionally until `until` is on screen.
    Scroll {
        direction: Direction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<Target>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until: Option<Target>,
    },
    /// Android key code name without the `KEYCODE_` prefix, e.g. `BACK`.
    Key {
        name: String,
    },
}

/// Which content to reveal: `Down` shows what is below (the finger moves up).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Serialize)]
pub struct ActOutcome {
    /// What was done, e.g. `tap e5 "Network & internet"`.
    pub action: String,
    pub settled: bool,
    /// The screen changed so much that the full tree is reported instead of a diff.
    pub new_screen: bool,
    pub screen: ScreenInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<TreeDiff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<UiTree>,
    /// The compact text form agents read.
    pub text: String,
}
