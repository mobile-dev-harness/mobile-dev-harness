//! Observation: turns what a device shows into compact, agent-friendly data.
//!
//! UI trees today; logs and crash reports land here too (M1).
//!
//! Pipeline: [`compress`] a backend's [`RawNode`](mdh_core::ui::RawNode)s into a [`UiTree`], let a
//! [`RefTable`] assign session-stable refs, then [`render`] it or [`diff`] it against the previous tree.

mod compress;
mod diff;
mod hash;
mod image;
mod logs;
mod refs;
mod render;
mod tree;

pub use compress::compress;
pub use diff::{Change, Field, Removed, TreeDiff, diff};
pub use image::{Jpeg, screenshot_jpeg};
pub use logs::{AppFilter, CrashKind, CrashReport, LogDigest, digest, render_logs};
pub use refs::RefTable;
pub use render::{render, render_diff, render_line, render_opaque, render_screen};
pub use tree::{OpaqueReason, OpaqueRegion, Role, State, UiNode, UiTree};
