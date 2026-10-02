//! The compact text format agents read (functional design §4.4).

use std::fmt::Write;

use mdh_core::ui::{Rect, ScreenInfo};

use crate::diff::{Field, TreeDiff, checked_word};
use crate::tree::{OpaqueReason, Role, UiNode, UiTree};

const MAX_TEXT_CHARS: usize = 120;
const MAX_LIST_ITEMS: usize = 30;

pub fn render(tree: &UiTree) -> String {
    let mut out = String::new();
    for node in &tree.nodes {
        render_node(&mut out, node, 0);
    }
    for region in &tree.opaque {
        let what = match region.reason {
            OpaqueReason::Webview => "webview",
            OpaqueReason::Undescribed => "undescribed region",
        };
        let _ = writeln!(
            out,
            "! {what} at {} has no readable content; take a screenshot to see it",
            rect(&region.bounds)
        );
    }
    out.truncate(out.trim_end().len());
    out
}

fn render_node(out: &mut String, node: &UiNode, depth: usize) {
    let _ = writeln!(out, "{:indent$}{}", "", line(node), indent = depth * 2);
    let shown = if node.role == Role::List {
        node.children.len().min(MAX_LIST_ITEMS)
    } else {
        node.children.len()
    };
    for child in &node.children[..shown] {
        render_node(out, child, depth + 1);
    }
    let hidden = node.children.len() - shown;
    if hidden > 0 {
        let _ = writeln!(
            out,
            "{:indent$}… {hidden} more",
            "",
            indent = (depth + 1) * 2
        );
    }
}

/// One node without its children: `[e4] button "Sign in" · "detail" disabled #login`.
pub(crate) fn line(node: &UiNode) -> String {
    let mut s = format!("[{}]", node.r#ref);
    // Plain text is the most common node; its role is implied by the bare label.
    if node.role != Role::Text {
        s.push(' ');
        s.push_str(node.role.as_str());
    }
    if let Some(label) = &node.label {
        s.push(' ');
        s.push_str(&quote(label));
    }
    if let Some(detail) = &node.detail {
        s.push_str(" · ");
        s.push_str(&quote(detail));
    }
    match (&node.value, node.role) {
        (Some(_), _) if node.state.password => s.push_str(" value=••••"),
        (Some(value), _) => {
            s.push_str(" value=");
            s.push_str(&quote(value));
        }
        (None, Role::Textbox) => s.push_str(" empty"),
        (None, _) => {}
    }
    if let Some(checked) = node.state.checked {
        s.push(' ');
        s.push_str(checked_word(node.role, checked));
    }
    for (on, word) in [
        (node.state.disabled, "disabled"),
        (node.state.selected, "selected"),
        (node.state.focused, "focused"),
        (node.state.scrollable, "scrollable"),
    ] {
        if on {
            s.push(' ');
            s.push_str(word);
        }
    }
    if let Some(id) = &node.id {
        if node.role != Role::Text {
            s.push_str(" #");
            s.push_str(id);
        }
    }
    s
}

/// `screen com.example/.LoginActivity  1080x2400  keyboard  overlay:com.android.permissioncontroller`
pub fn render_screen(screen: &ScreenInfo) -> String {
    let mut line = format!(
        "screen {}  {}x{}",
        screen.activity.as_deref().unwrap_or("?"),
        screen.size.width(),
        screen.size.height()
    );
    if screen.keyboard {
        line.push_str("  keyboard");
    }
    if let Some(overlay) = &screen.overlay {
        line.push_str("  overlay:");
        line.push_str(overlay);
    }
    line
}

/// `+` added, `~` changed, `-` removed (refs merged into ranges).
pub fn render_diff(d: &TreeDiff) -> String {
    let mut lines: Vec<String> = d.added.iter().map(|n| format!("+ {}", line(n))).collect();
    for c in &d.changed {
        let mut s = format!("~ [{}] {}", c.r#ref, c.role.as_str());
        if let Some(label) = &c.label {
            s.push(' ');
            s.push_str(&quote(label));
        }
        let field = match c.field {
            Field::Value => "value ",
            Field::Detail => "detail ",
            Field::Checked | Field::Enabled | Field::Selected | Field::Focused => "",
        };
        let _ = write!(s, ": {field}{} → {}", c.from, c.to);
        lines.push(s);
    }
    if !d.removed.is_empty() {
        lines.push(format!(
            "- {}",
            ref_ranges(d.removed.iter().map(|r| r.r#ref.as_str()))
        ));
    }
    lines.join("\n")
}

/// `e1 e2 e3 e7` → `e1..e3, e7`.
fn ref_ranges<'a>(refs: impl Iterator<Item = &'a str>) -> String {
    let mut nums: Vec<u32> = refs
        .filter_map(|r| r.strip_prefix('e')?.parse().ok())
        .collect();
    nums.sort_unstable();
    nums.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < nums.len() {
        let start = nums[i];
        while i + 1 < nums.len() && nums[i + 1] == nums[i] + 1 {
            i += 1;
        }
        parts.push(if nums[i] == start {
            format!("e{start}")
        } else {
            format!("e{start}..e{}", nums[i])
        });
        i += 1;
    }
    parts.join(", ")
}

fn quote(s: &str) -> String {
    let mut out = String::from('"');
    for (i, c) in s.chars().enumerate() {
        if i == MAX_TEXT_CHARS {
            out.push('…');
            break;
        }
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn rect(r: &Rect) -> String {
    format!("[{},{}][{},{}]", r.left, r.top, r.right, r.bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_merge_consecutive_refs() {
        let refs = ["e7", "e1", "e2", "e3", "e9", "e10"];
        assert_eq!(ref_ranges(refs.into_iter()), "e1..e3, e7, e9..e10");
    }

    #[test]
    fn quote_escapes_and_truncates() {
        assert_eq!(quote("a \"b\"\nc"), r#""a \"b\"\nc""#);
        let long = "x".repeat(200);
        assert_eq!(quote(&long).chars().count(), MAX_TEXT_CHARS + 3);
    }
}
