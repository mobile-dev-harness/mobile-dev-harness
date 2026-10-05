use std::collections::HashMap;

use mdh_core::ui::{RawNode, Rect};

use crate::hash::Fnv;
use crate::tree::{OpaqueReason, OpaqueRegion, Role, State, UiNode, UiTree};

/// Clickable containers covering more than 1/LAYOUT_SHARE of the screen are layout, not controls
/// (e.g. a clickable root view); their children are kept, the container is dropped.
const LAYOUT_SHARE: i64 = 2;
/// Undescribed leaves covering at least 1/OPAQUE_SHARE of the screen are reported as opaque.
const OPAQUE_SHARE: i64 = 10;
/// An undescribed region stops counting as opaque once readable leaves cover 1/COVERED_SHARE of it
/// (e.g. a full-screen background or scrim behind the actual content).
const COVERED_SHARE: i64 = 5;
/// At most this many texts are folded into an interactive node's label and detail.
const MAX_ABSORBED: usize = 4;

/// Compresses a raw hierarchy: prunes invisible and meaningless nodes, infers roles, folds texts
/// into the controls they describe and computes the stable keys refs are derived from.
pub fn compress(roots: &[RawNode]) -> UiTree {
    let screen = roots
        .iter()
        .map(|r| r.bounds)
        .reduce(|a, b| a.union(&b))
        .unwrap_or_default();
    let mut compressor = Compressor {
        screen,
        opaque: Vec::new(),
        order: subtree_ranges(roots),
        covers: app_covers(roots, screen),
    };
    let mut nodes: Vec<UiNode> = roots
        .iter()
        .flat_map(|r| compressor.visit(r, false))
        .collect();
    assign_keys(&mut nodes, 0);
    let mut opaque = compressor.opaque;
    let leaves: Vec<Rect> = leaf_bounds(&nodes);
    opaque.retain(|r| r.reason == OpaqueReason::Webview || !mostly_covered(&r.bounds, &leaves));
    UiTree {
        screen,
        nodes,
        opaque,
        raw_nodes: roots.iter().map(count).sum(),
    }
}

fn leaf_bounds(nodes: &[UiNode]) -> Vec<Rect> {
    let mut out = Vec::new();
    for n in nodes {
        if n.children.is_empty() {
            out.push(n.bounds);
        } else {
            out.extend(leaf_bounds(&n.children));
        }
    }
    out
}

fn mostly_covered(region: &Rect, leaves: &[Rect]) -> bool {
    let covered: i64 = leaves
        .iter()
        .filter_map(|l| l.intersect(region))
        .map(|r| r.area())
        .sum();
    covered * COVERED_SHARE >= region.area()
}

fn count(node: &RawNode) -> usize {
    1 + node.children.iter().map(count).sum::<usize>()
}

struct Compressor {
    screen: Rect,
    opaque: Vec<OpaqueRegion>,
    /// Raw node → (pre-order index, end of its subtree).
    order: HashMap<*const RawNode, (usize, usize)>,
    covers: Vec<Cover>,
}

/// A part of the app that is drawn after (on top of) everything before it in tree order.
struct Cover {
    bounds: Rect,
    start: usize,
}

/// Pre-order index and subtree end of every raw node, keyed by address (the roots are borrowed
/// for the whole compression).
fn subtree_ranges(roots: &[RawNode]) -> HashMap<*const RawNode, (usize, usize)> {
    fn walk(n: &RawNode, next: &mut usize, out: &mut HashMap<*const RawNode, (usize, usize)>) {
        let start = *next;
        *next += 1;
        for c in &n.children {
            walk(c, next, out);
        }
        out.insert(n, (start, *next));
    }
    let mut out = HashMap::new();
    let mut next = 0;
    for r in roots {
        walk(r, &mut next, &mut out);
    }
    out
}

/// Containers that cover what was drawn before them: they have content of their own and take
/// less than half of the screen (action bars, toolbars, bottom bars, floating buttons). Larger
/// containers are usually transparent overlays and would mark everything as covered. Child order
/// stands in for drawing order, which holds for the common layouts.
fn app_covers(roots: &[RawNode], screen: Rect) -> Vec<Cover> {
    fn has_content(n: &RawNode) -> bool {
        n.text.is_some()
            || n.desc.is_some()
            || n.flags.clickable
            || n.children.iter().any(has_content)
    }
    fn walk(n: &RawNode, next: &mut usize, screen: Rect, out: &mut Vec<Cover>) {
        let start = *next;
        *next += 1;
        if let Some(bounds) = n.bounds.intersect(&screen)
            && !n.children.is_empty()
            && bounds.area() * 2 < screen.area()
            && has_content(n)
        {
            out.push(Cover { bounds, start });
        }
        for c in &n.children {
            walk(c, next, screen, out);
        }
    }
    let mut out = Vec::new();
    let mut next = 0;
    for r in roots {
        walk(r, &mut next, screen, &mut out);
    }
    out
}

impl Compressor {
    /// Covers drawn after `raw`'s whole subtree that overlap it.
    fn covers_of(&self, raw: &RawNode, visible: Rect) -> Vec<Rect> {
        let Some(&(_, end)) = self.order.get(&(raw as *const RawNode)) else {
            return Vec::new();
        };
        self.covers
            .iter()
            .filter(|c| c.start >= end)
            .filter_map(|c| c.bounds.intersect(&visible))
            .collect()
    }

    /// Returns the compact nodes standing in for `raw`: itself, its lifted children, or nothing.
    fn visit(&mut self, raw: &RawNode, in_list: bool) -> Vec<UiNode> {
        let Some(visible) = raw.bounds.intersect(&self.screen) else {
            return Vec::new();
        };
        let role = infer_role(raw, in_list);
        let covered_by = self.covers_of(raw, visible);
        let interactive = is_interactive(raw, role);
        // List membership flows through plain containers down to the first interactive node.
        let child_in_list = role == Role::List || (in_list && !interactive);
        let mut children: Vec<UiNode> = raw
            .children
            .iter()
            .flat_map(|c| self.visit(c, child_in_list))
            .collect();

        match role {
            // A WebView nested directly in another adds nothing; keep the inner one.
            Role::Webview if children.len() == 1 && children[0].role == Role::Webview => {
                return children;
            }
            Role::Webview => {
                // The system WebView exposes the page through accessibility; only an empty one
                // (still loading, canvas-drawn, or a WebView without accessibility) is opaque.
                if children.is_empty() {
                    self.opaque.push(OpaqueRegion {
                        bounds: visible,
                        reason: OpaqueReason::Webview,
                    });
                }
                let mut node = new_node(role, raw, visible);
                node.covered_by = covered_by.clone();
                node.label = raw.desc.clone();
                node.children = children;
                return vec![node];
            }
            Role::List if children.is_empty() => return Vec::new(),
            // Scroll containers nested directly in each other add nothing; keep the innermost.
            Role::List
                if raw.desc.is_none() && children.len() == 1 && children[0].role == Role::List =>
            {
                return children;
            }
            Role::List => {
                let mut node = new_node(role, raw, visible);
                node.covered_by = covered_by.clone();
                node.label = raw.desc.clone();
                node.state.scrollable = true;
                node.children = children;
                return vec![node];
            }
            _ => {}
        }

        if interactive {
            let is_layout = !role.is_toggle()
                && role != Role::Textbox
                && !children.is_empty()
                && visible.area() * LAYOUT_SHARE > self.screen.area();
            if is_layout {
                return children;
            }
            let mut node = new_node(role, raw, visible);
            node.covered_by = covered_by.clone();
            fill_labels(&mut node, raw);
            merge_toggle(&mut node, &mut children);
            absorb_texts(&mut node, &mut children);
            drop_nested_twin(&node, &mut children);
            node.children = children;
            return vec![node];
        }

        if role == Role::Progress {
            let mut node = new_node(role, raw, visible);
            node.covered_by = covered_by;
            return vec![node];
        }

        let label = raw.text.clone().or_else(|| raw.desc.clone());
        match label {
            Some(label) if children.is_empty() => {
                let mut node = new_node(
                    if role == Role::Image {
                        Role::Image
                    } else {
                        Role::Text
                    },
                    raw,
                    visible,
                );
                node.covered_by = covered_by.clone();
                node.label = Some(label);
                vec![node]
            }
            Some(label) => {
                let mut node = new_node(Role::Group, raw, visible);
                node.covered_by = covered_by.clone();
                node.label = Some(label);
                node.children = children;
                vec![node]
            }
            None => {
                // Empty generic views and layouts are spacers and backgrounds (observed: a spacer
                // reported as unreadable); drawn content comes from custom views, SurfaceView,
                // TextureView or images.
                let short_class = raw.class.rsplit('.').next().unwrap_or(&raw.class);
                let spacer = matches!(
                    short_class,
                    "View"
                        | "Space"
                        | "FrameLayout"
                        | "LinearLayout"
                        | "RelativeLayout"
                        | "ViewGroup"
                        | "ConstraintLayout"
                );
                let undescribed_leaf = raw.children.is_empty()
                    && !spacer
                    && visible.area() * OPAQUE_SHARE >= self.screen.area();
                if undescribed_leaf {
                    self.opaque.push(OpaqueRegion {
                        bounds: visible,
                        reason: OpaqueReason::Undescribed,
                    });
                }
                children
            }
        }
    }
}

fn infer_role(raw: &RawNode, in_list: bool) -> Role {
    // Short class name, e.g. `TabLayout$TabView` for `com.google.android.material.tabs.TabLayout$TabView`.
    let class = raw.class.rsplit('.').next().unwrap_or(&raw.class);
    let has = |s: &str| class.contains(s);

    // A Compose `toggleable(role = Role.Switch)` row is a checkable View whose role description
    // sits on a child with the same bounds (observed), so look one level down for checkable nodes.
    let description = raw.role_description.as_deref().or_else(|| {
        raw.flags
            .checkable
            .then(|| {
                raw.children
                    .iter()
                    .find_map(|c| c.role_description.as_deref())
            })
            .flatten()
    });
    if let Some(role) = description.and_then(role_from_description) {
        return role;
    }
    if has("WebView") {
        Role::Webview
    } else if has("EditText") || has("AutoCompleteTextView") {
        Role::Textbox
    } else if has("Switch") || has("ToggleButton") {
        Role::Switch
    } else if has("CheckBox") || has("CheckedTextView") {
        Role::Checkbox
    } else if has("RadioButton") {
        Role::Radio
    } else if has("SeekBar") || has("Slider") || has("RatingBar") {
        Role::Slider
    } else if has("ProgressBar") {
        Role::Progress
    } else if has("TabView") {
        Role::Tab
    } else if raw.flags.scrollable
        || [
            "RecyclerView",
            "ListView",
            "GridView",
            "ScrollView",
            "ViewPager",
        ]
        .iter()
        .any(|c| has(c))
    {
        Role::List
    } else if has("Button") {
        Role::Button
    } else if raw.flags.checkable {
        Role::Checkbox
    } else if raw.flags.clickable || raw.flags.long_clickable {
        if in_list { Role::Item } else { Role::Button }
    } else if has("Image") {
        Role::Image
    } else {
        Role::Text
    }
}

/// Roles toolkits announce in English role descriptions; anything else falls back to the class.
fn role_from_description(description: &str) -> Option<Role> {
    match description.to_lowercase().as_str() {
        "switch" | "toggle" => Some(Role::Switch),
        "checkbox" | "check box" => Some(Role::Checkbox),
        "radio button" => Some(Role::Radio),
        "button" => Some(Role::Button),
        "tab" => Some(Role::Tab),
        "image" => Some(Role::Image),
        _ => None,
    }
}

fn is_interactive(raw: &RawNode, role: Role) -> bool {
    raw.flags.clickable
        || raw.flags.long_clickable
        || raw.flags.checkable
        || matches!(role, Role::Textbox | Role::Slider)
}

fn new_node(role: Role, raw: &RawNode, bounds: Rect) -> UiNode {
    UiNode {
        r#ref: String::new(),
        key: 0,
        role,
        label: None,
        detail: None,
        value: None,
        id: short_id(raw.resource_id.as_deref()),
        state: State {
            disabled: !raw.flags.enabled,
            checked: raw.flags.checkable.then_some(raw.flags.checked),
            selected: raw.flags.selected,
            focused: raw.flags.focused,
            scrollable: false,
            password: raw.flags.password,
            obscured: false,
        },
        bounds,
        covered_by: Vec::new(),
        children: Vec::new(),
    }
}

/// `com.example:id/login_button` → `login_button`. Framework ids such as `android:id/title` carry no
/// meaning for the app under test and are dropped.
fn short_id(resource_id: Option<&str>) -> Option<String> {
    let id = resource_id?;
    if id.starts_with("android:") {
        return None;
    }
    Some(
        id.rsplit_once(":id/")
            .map_or(id, |(_, name)| name)
            .to_owned(),
    )
}

fn fill_labels(node: &mut UiNode, raw: &RawNode) {
    if node.role == Role::Textbox {
        node.label = raw.hint.clone().or_else(|| raw.desc.clone());
        // Some Android versions report the hint as the text of an empty field.
        node.value = raw.text.clone().filter(|t| Some(t) != raw.hint.as_ref());
        return;
    }
    match (&raw.text, &raw.desc) {
        (Some(text), Some(desc)) if text != desc => {
            node.label = Some(text.clone());
            node.detail = Some(desc.clone());
        }
        (text, desc) => node.label = text.clone().or_else(|| desc.clone()),
    }
}

fn is_control(node: &UiNode) -> bool {
    !matches!(
        node.role,
        Role::Text | Role::Image | Role::Group | Role::Progress
    )
}

/// A row whose only control is a switch/checkbox becomes that toggle: tapping the row toggles it,
/// and one node reads better than a row plus a nameless switch.
fn merge_toggle(node: &mut UiNode, children: &mut Vec<UiNode>) {
    if !matches!(node.role, Role::Button | Role::Item) || node.state.checked.is_some() {
        return;
    }
    let mut controls = children.iter().enumerate().filter(|(_, c)| is_control(c));
    let (Some((i, toggle)), None) = (controls.next(), controls.next()) else {
        return;
    };
    if !toggle.role.is_toggle() || !toggle.children.is_empty() {
        return;
    }
    let toggle = children.remove(i);
    node.role = toggle.role;
    node.state.checked = toggle.state.checked;
    node.state.disabled |= toggle.state.disabled;
    node.label = node.label.take().or(toggle.label);
    node.id = node.id.take().or(toggle.id);
}

/// A toggle around a toggle of the same role, name and state is one control (a Compose chip that
/// is selectable around its own checkbox): two would make its name match twice.
fn drop_nested_twin(node: &UiNode, children: &mut Vec<UiNode>) {
    if !node.role.is_toggle() {
        return;
    }
    children.retain(|c| {
        let twin = c.role == node.role
            && c.children.is_empty()
            && c.state.checked == node.state.checked
            && (c.label.is_none() || c.label == node.label);
        !twin
    });
}

/// Moves plain text and image leaves into the node's label and detail. A text field only takes a
/// label this way, and only when it has none: Compose renders a field's label as a child text.
fn absorb_texts(node: &mut UiNode, children: &mut Vec<UiNode>) {
    if node.role == Role::Textbox {
        if node.label.is_none()
            && let Some(i) = children
                .iter()
                .position(|c| c.role == Role::Text && c.children.is_empty())
        {
            node.label = children.remove(i).label;
        }
        return;
    }
    let mut texts = Vec::new();
    children.retain(|c| {
        let absorbable = c.children.is_empty()
            && (c.role == Role::Text || (c.role == Role::Image && c.label.is_some()));
        if absorbable && texts.len() < MAX_ABSORBED {
            texts.extend(c.label.clone());
            false
        } else {
            true
        }
    });
    for text in texts {
        let duplicate = node.label.as_ref() == Some(&text) || node.detail.as_ref() == Some(&text);
        if duplicate {
            continue;
        }
        if node.label.is_none() {
            node.label = Some(text);
        } else if let Some(detail) = &mut node.detail {
            detail.push_str(" · ");
            detail.push_str(&text);
        } else {
            node.detail = Some(text);
        }
    }
}

/// Key = hash(parent key, role, id, label, occurrence among identical siblings). Values and states
/// are excluded so typing or toggling keeps an element's identity; text content is included because
/// list rows often share one resource id.
fn assign_keys(nodes: &mut [UiNode], parent: u64) {
    let mut seen: HashMap<u64, u64> = HashMap::new();
    for node in nodes {
        let mut h = Fnv::new();
        h.u64(parent);
        h.field(node.role.as_str());
        h.field(node.id.as_deref().unwrap_or_default());
        h.field(node.label.as_deref().unwrap_or_default());
        let base = h.0;
        let occurrence = seen.entry(base).or_insert(0);
        h.u64(*occurrence);
        *occurrence += 1;
        node.key = h.0;
        assign_keys(&mut node.children, node.key);
    }
}
