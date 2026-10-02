use serde::{Deserialize, Serialize};

/// Coordinate-level input, the lowest layer of interaction. Element targeting happens above it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Input {
    Tap {
        x: i32,
        y: i32,
    },
    /// A long press is a swipe that doesn't move.
    Swipe {
        from: (i32, i32),
        to: (i32, i32),
        duration_ms: u32,
    },
    /// Android key code name without the `KEYCODE_` prefix, e.g. `BACK`, `HOME`, `ENTER`.
    Key {
        name: String,
    },
    /// Replaces the content of the focused text field. Any Unicode text.
    SetText {
        text: String,
    },
}
