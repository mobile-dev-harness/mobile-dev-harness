use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::tree::{Role, UiNode, UiTree};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TreeDiff {
    /// New nodes in pre-order, without their children (those are listed individually).
    pub added: Vec<UiNode>,
    pub changed: Vec<Change>,
    pub removed: Vec<Removed>,
}

impl TreeDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub r#ref: String,
    pub role: Role,
    pub label: Option<String>,
    pub field: Field,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Value,
    Detail,
    Checked,
    Enabled,
    Selected,
    Focused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub r#ref: String,
    pub role: Role,
    pub label: Option<String>,
}

/// Matches nodes by stable key. Both trees must have refs from the same [`RefTable`](crate::RefTable).
pub fn diff(old: &UiTree, new: &UiTree) -> TreeDiff {
    let old_by_key: HashMap<u64, &UiNode> = old.iter().map(|n| (n.key, n)).collect();
    let new_keys: HashSet<u64> = new.iter().map(|n| n.key).collect();

    let mut d = TreeDiff::default();
    for node in new.iter() {
        match old_by_key.get(&node.key) {
            None => d.added.push(UiNode {
                children: Vec::new(),
                ..node.clone()
            }),
            Some(before) => compare(before, node, &mut d.changed),
        }
    }
    d.removed = old
        .iter()
        .filter(|n| !new_keys.contains(&n.key))
        .map(|n| Removed {
            r#ref: n.r#ref.clone(),
            role: n.role,
            label: n.label.clone(),
        })
        .collect();
    d
}

fn compare(old: &UiNode, new: &UiNode, out: &mut Vec<Change>) {
    let mut push = |field, from: String, to: String| {
        if from != to {
            out.push(Change {
                r#ref: new.r#ref.clone(),
                role: new.role,
                label: new.label.clone(),
                field,
                from,
                to,
            });
        }
    };
    let text = |v: &Option<String>, masked: bool| match v {
        Some(_) if masked => "••••".to_owned(),
        Some(v) => format!("{v:?}"),
        None => "empty".to_owned(),
    };
    let masked = new.state.password;
    push(
        Field::Value,
        text(&old.value, masked),
        text(&new.value, masked),
    );
    push(
        Field::Detail,
        text(&old.detail, false),
        text(&new.detail, false),
    );
    if let (Some(a), Some(b)) = (old.state.checked, new.state.checked) {
        let word = |on: bool| checked_word(new.role, on).to_owned();
        push(Field::Checked, word(a), word(b));
    }
    let word = |b: bool, yes: &str, no: &str| if b { yes } else { no }.to_owned();
    push(
        Field::Enabled,
        word(old.state.disabled, "disabled", "enabled"),
        word(new.state.disabled, "disabled", "enabled"),
    );
    push(
        Field::Selected,
        word(old.state.selected, "selected", "unselected"),
        word(new.state.selected, "selected", "unselected"),
    );
    push(
        Field::Focused,
        word(old.state.focused, "focused", "unfocused"),
        word(new.state.focused, "focused", "unfocused"),
    );
}

pub(crate) fn checked_word(role: Role, checked: bool) -> &'static str {
    match (role, checked) {
        (Role::Switch, true) => "on",
        (Role::Switch, false) => "off",
        (_, true) => "checked",
        (_, false) => "unchecked",
    }
}
