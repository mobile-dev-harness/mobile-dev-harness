use mdh_core::ui::Rect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Button,
    /// A tappable row or cell inside a list.
    Item,
    Textbox,
    Checkbox,
    Switch,
    Radio,
    Slider,
    Progress,
    Image,
    List,
    Tab,
    Text,
    Webview,
    /// A non-interactive container that carries a description of its own.
    Group,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Item => "item",
            Role::Textbox => "textbox",
            Role::Checkbox => "checkbox",
            Role::Switch => "switch",
            Role::Radio => "radio",
            Role::Slider => "slider",
            Role::Progress => "progress",
            Role::Image => "image",
            Role::List => "list",
            Role::Tab => "tab",
            Role::Text => "text",
            Role::Webview => "webview",
            Role::Group => "group",
        }
    }

    pub fn is_toggle(self) -> bool {
        matches!(self, Role::Checkbox | Role::Switch | Role::Radio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct State {
    pub disabled: bool,
    /// `Some` only for checkable nodes.
    pub checked: Option<bool>,
    pub selected: bool,
    pub focused: bool,
    pub scrollable: bool,
    pub password: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiNode {
    /// Session-stable handle such as `e12`; empty until a [`RefTable`](crate::RefTable) assigns it.
    pub r#ref: String,
    /// Identity used to keep refs stable across observations; see `compress::assign_keys`.
    pub key: u64,
    pub role: Role,
    pub label: Option<String>,
    /// Secondary text, e.g. the summary line under a settings title.
    pub detail: Option<String>,
    /// Current content of inputs.
    pub value: Option<String>,
    /// Resource id without the package prefix; generic `android:id/*` ids are dropped.
    pub id: Option<String>,
    pub state: State,
    pub bounds: Rect,
    pub children: Vec<UiNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpaqueReason {
    Webview,
    /// A large node with no text, description or accessible children (canvas, image, custom view,
    /// Compose without semantics).
    Undescribed,
}

/// A screen area whose content the tree can't describe; a screenshot is needed to see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpaqueRegion {
    pub bounds: Rect,
    pub reason: OpaqueReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UiTree {
    pub screen: Rect,
    pub nodes: Vec<UiNode>,
    pub opaque: Vec<OpaqueRegion>,
    /// Number of nodes in the raw hierarchy, for compression statistics.
    pub raw_nodes: usize,
}

impl UiTree {
    /// Pre-order iteration over all nodes.
    pub fn iter(&self) -> impl Iterator<Item = &UiNode> {
        let mut stack: Vec<&UiNode> = self.nodes.iter().rev().collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }

    pub fn find(&self, r#ref: &str) -> Option<&UiNode> {
        self.iter().find(|n| n.r#ref == r#ref)
    }
}
