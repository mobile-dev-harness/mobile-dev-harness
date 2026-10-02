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

    pub const ALL: [Role; 14] = [
        Role::Button,
        Role::Item,
        Role::Textbox,
        Role::Checkbox,
        Role::Switch,
        Role::Radio,
        Role::Slider,
        Role::Progress,
        Role::Image,
        Role::List,
        Role::Tab,
        Role::Text,
        Role::Webview,
        Role::Group,
    ];

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// Something the user can operate, as opposed to content and containers.
    pub fn is_control(self) -> bool {
        !matches!(
            self,
            Role::Text | Role::Image | Role::Group | Role::Progress | Role::List | Role::Webview
        )
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
    /// Mostly covered by system windows (see `mark_obscured`).
    #[serde(default)]
    pub obscured: bool,
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
    /// Parts of the app drawn on top of this node (an action bar over edge-to-edge content, a
    /// bottom bar, …); see `compress::app_covers`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covered_by: Vec<Rect>,
    pub children: Vec<UiNode>,
}

impl UiNode {
    /// The largest part of the node left visible by the app's own overlays and `system` windows.
    pub fn visible_part(&self, system: &[Rect]) -> Option<Rect> {
        let covers: Vec<Rect> = system.iter().chain(&self.covered_by).copied().collect();
        self.bounds.largest_visible_part(&covers)
    }
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

    /// Marks nodes of which less than half is visible, given the app's own overlays and the
    /// `system` windows (status and navigation bars, keyboard).
    pub fn mark_obscured(&mut self, system: &[Rect]) {
        fn mark(nodes: &mut [UiNode], system: &[Rect]) {
            for n in nodes {
                let visible = n.visible_part(system).map_or(0, |r| r.area());
                n.state.obscured = visible * 2 < n.bounds.area();
                mark(&mut n.children, system);
            }
        }
        mark(&mut self.nodes, system);
    }

    pub fn find(&self, r#ref: &str) -> Option<&UiNode> {
        self.iter().find(|n| n.r#ref == r#ref)
    }

    /// Changes whenever anything visible changes: identity, position, content or state. Used to
    /// decide when the UI has settled.
    pub fn fingerprint(&self) -> u64 {
        let mut h = crate::hash::Fnv::new();
        for n in self.iter() {
            h.u64(n.key);
            for v in [n.bounds.left, n.bounds.top, n.bounds.right, n.bounds.bottom] {
                h.u64(v as u64);
            }
            h.field(n.value.as_deref().unwrap_or_default());
            h.field(n.detail.as_deref().unwrap_or_default());
            let s = n.state;
            h.u64(u64::from(s.disabled) | u64::from(s.focused) << 1 | u64::from(s.selected) << 2);
            h.u64(s.checked.map_or(2, u64::from));
        }
        h.0
    }
}
