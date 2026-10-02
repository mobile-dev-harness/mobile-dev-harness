//! Turns raw UI hierarchies into compact, agent-friendly trees with stable refs and diffs.
//!
//! Pipeline: [`compress`] a backend's [`RawNode`](mdh_core::ui::RawNode)s into a [`UiTree`], let a
//! [`RefTable`] assign session-stable refs, then [`render`] it or [`diff`] it against the previous tree.

mod compress;
mod diff;
mod refs;
mod render;
mod tree;

pub use compress::compress;
pub use diff::{Change, Field, Removed, TreeDiff, diff};
pub use refs::RefTable;
pub use render::{render, render_diff};
pub use tree::{OpaqueReason, OpaqueRegion, Role, State, UiNode, UiTree};
