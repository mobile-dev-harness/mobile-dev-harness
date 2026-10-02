use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::tree::{UiNode, UiTree};

/// Hands out `e1`, `e2`, … and keeps the same ref for the same element key for the whole session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefTable {
    by_key: HashMap<u64, u32>,
    next: u32,
}

impl RefTable {
    /// How many distinct elements have received a ref in this session.
    pub fn assigned(&self) -> usize {
        self.by_key.len()
    }

    pub fn assign(&mut self, tree: &mut UiTree) {
        for node in &mut tree.nodes {
            self.assign_node(node);
        }
    }

    fn assign_node(&mut self, node: &mut UiNode) {
        let next = &mut self.next;
        let n = *self.by_key.entry(node.key).or_insert_with(|| {
            *next += 1;
            *next
        });
        node.r#ref = format!("e{n}");
        for child in &mut node.children {
            self.assign_node(child);
        }
    }
}
