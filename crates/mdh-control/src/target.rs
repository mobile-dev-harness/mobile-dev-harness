//! What an action is aimed at, and finding it on screen.

use std::fmt;

use mdh_core::{Error, Result};
use mdh_observe::{Role, UiNode, UiTree, render_line};
use serde::{Deserialize, Serialize};

const MAX_CANDIDATES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// A session-scoped ref such as `e12`.
    Ref(String),
    Point {
        x: i32,
        y: i32,
    },
    /// Persistable; what recordings and flows use.
    Selector(Selector),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Selector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// Which of several matches, in screen order; a last resort for identical elements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMatch {
    /// The label equals the text.
    Exact(String),
    /// Label, detail or value contains the text, ignoring case.
    Contains(String),
    /// A bare label typed by an agent: exact, then case-insensitive, then contains.
    Label(String),
}

impl Target {
    /// Parses the CLI form: `e12`, `100,200`, `id=…;text=…;text~=…;role=…;index=…`, or a bare label.
    pub fn parse(s: &str) -> Result<Target> {
        let t = s.trim();
        if t.is_empty() {
            return Err(invalid(s, "empty"));
        }
        if is_ref(t) {
            return Ok(Target::Ref(t.to_owned()));
        }
        if let Some((x, y)) = t.split_once(',') {
            if let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse()) {
                return Ok(Target::Point { x, y });
            }
        }
        let first_key = t.split_once('=').map(|(k, _)| k.trim());
        if matches!(first_key, Some("id" | "text" | "text~" | "role" | "index")) {
            return parse_selector(t).map(Target::Selector);
        }
        Ok(Target::Selector(Selector {
            text: Some(TextMatch::Label(t.to_owned())),
            ..Selector::default()
        }))
    }
}

fn is_ref(s: &str) -> bool {
    s.strip_prefix('e')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn parse_selector(s: &str) -> Result<Selector> {
    let mut sel = Selector::default();
    for clause in s.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let (key, value) = clause
            .split_once('=')
            .ok_or_else(|| invalid(s, &format!("`{clause}` is not `key=value`")))?;
        let value = value.trim().to_owned();
        match key.trim() {
            "id" => sel.id = Some(value),
            "text" => sel.text = Some(TextMatch::Exact(value)),
            "text~" => sel.text = Some(TextMatch::Contains(value)),
            "role" => {
                sel.role = Some(
                    Role::parse(&value)
                        .ok_or_else(|| invalid(s, &format!("unknown role `{value}`")))?,
                )
            }
            "index" => {
                sel.index = Some(
                    value
                        .parse()
                        .map_err(|_| invalid(s, "index must be a number"))?,
                )
            }
            other => return Err(invalid(s, &format!("unknown key `{other}`"))),
        }
    }
    Ok(sel)
}

fn invalid(target: &str, reason: &str) -> Error {
    Error::InvalidTarget {
        target: target.to_owned(),
        reason: reason.to_owned(),
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Ref(r) => f.write_str(r),
            Target::Point { x, y } => write!(f, "{x},{y}"),
            Target::Selector(sel) => sel.fmt(f),
        }
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut clauses = Vec::new();
        if let Some(role) = self.role {
            clauses.push(format!("role={}", role.as_str()));
        }
        if let Some(id) = &self.id {
            clauses.push(format!("id={id}"));
        }
        match &self.text {
            Some(TextMatch::Exact(t)) => clauses.push(format!("text={t}")),
            Some(TextMatch::Contains(t)) => clauses.push(format!("text~={t}")),
            Some(TextMatch::Label(t)) if clauses.is_empty() && self.index.is_none() => {
                return f.write_str(t);
            }
            Some(TextMatch::Label(t)) => clauses.push(format!("text={t}")),
            None => {}
        }
        if let Some(i) = self.index {
            clauses.push(format!("index={i}"));
        }
        f.write_str(&clauses.join(";"))
    }
}

/// Where to act, and the element there if the target named one.
pub(crate) struct Resolved<'a> {
    pub node: Option<&'a UiNode>,
    pub point: (i32, i32),
}

/// Finds `target` in `tree`. A ref that no longer exists is looked up in `previous` (the tree the
/// agent last saw) and re-found through a selector derived from it.
pub(crate) fn resolve<'a>(
    target: &Target,
    tree: &'a UiTree,
    previous: Option<&UiTree>,
) -> Result<Resolved<'a>> {
    let node = match target {
        Target::Point { x, y } => {
            return Ok(Resolved {
                node: None,
                point: (*x, *y),
            });
        }
        Target::Ref(r) => match (tree.find(r), previous.and_then(|p| p.find(r))) {
            (Some(node), _) => node,
            (None, Some(old)) => resolve_selector(
                &selector_for(old, previous.expect("found in previous")),
                tree,
                r,
            )?,
            (None, None) => {
                return Err(Error::ElementNotFound {
                    target: r.clone(),
                    candidates: Vec::new(),
                });
            }
        },
        Target::Selector(sel) => resolve_selector(sel, tree, &sel.to_string())?,
    };
    Ok(Resolved {
        node: Some(node),
        point: node.bounds.center(),
    })
}

fn resolve_selector<'a>(sel: &Selector, tree: &'a UiTree, shown: &str) -> Result<&'a UiNode> {
    let matches = find_all(sel, tree);
    if let Some(i) = sel.index {
        return matches
            .get(i)
            .copied()
            .ok_or_else(|| Error::ElementNotFound {
                target: shown.to_owned(),
                candidates: matches.iter().map(|n| render_line(n)).collect(),
            });
    }
    match matches.as_slice() {
        [] => Err(Error::ElementNotFound {
            target: shown.to_owned(),
            candidates: closest(sel, tree),
        }),
        [only] => Ok(only),
        many => {
            // A title and the control it labels often share text; acting means the control.
            let mut controls = many.iter().filter(|n| n.role.is_control());
            match (controls.next(), controls.next()) {
                (Some(control), None) => Ok(control),
                _ => Err(Error::AmbiguousTarget {
                    target: shown.to_owned(),
                    candidates: many.iter().take(5).map(|n| render_line(n)).collect(),
                }),
            }
        }
    }
}

/// All nodes matching `sel`, in screen (pre-)order.
pub fn find_all<'a>(sel: &Selector, tree: &'a UiTree) -> Vec<&'a UiNode> {
    let base = |n: &&UiNode| {
        sel.id.as_ref().is_none_or(|id| n.id.as_ref() == Some(id))
            && sel.role.is_none_or(|r| n.role == r)
    };
    let label = |n: &UiNode| n.label.as_deref().unwrap_or_default().to_owned();
    let contains = |n: &UiNode, needle: &str| {
        let needle = needle.to_lowercase();
        [&n.label, &n.detail, &n.value]
            .into_iter()
            .flatten()
            .any(|t| t.to_lowercase().contains(&needle))
    };
    let filter = |pred: &dyn Fn(&UiNode) -> bool| -> Vec<&'a UiNode> {
        tree.iter().filter(base).filter(|n| pred(n)).collect()
    };
    match &sel.text {
        None => filter(&|_| true),
        Some(TextMatch::Exact(t)) => filter(&|n| label(n) == *t),
        Some(TextMatch::Contains(t)) => filter(&|n| contains(n, t)),
        Some(TextMatch::Label(t)) => {
            let tiers: [&dyn Fn(&UiNode) -> bool; 3] = [
                &|n| label(n) == *t,
                &|n| label(n).to_lowercase() == t.to_lowercase(),
                &|n| contains(n, t),
            ];
            tiers
                .into_iter()
                .map(|tier| filter(tier))
                .find(|m| !m.is_empty())
                .unwrap_or_default()
        }
    }
}

/// The on-screen elements most similar to what the selector asked for.
fn closest(sel: &Selector, tree: &UiTree) -> Vec<String> {
    let wanted = match &sel.text {
        Some(TextMatch::Exact(t) | TextMatch::Contains(t) | TextMatch::Label(t)) => t.as_str(),
        None => sel.id.as_deref().unwrap_or_default(),
    };
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, &UiNode)> = tree
        .iter()
        .map(|n| {
            let score = [&n.label, &n.id, &n.value]
                .into_iter()
                .flatten()
                .map(|t| similarity(wanted, t))
                .fold(0.0, f64::max);
            (score, n)
        })
        .filter(|(score, _)| *score >= 0.4)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(_, n)| render_line(n))
        .collect()
}

/// 1.0 for equal strings (ignoring case), high for containment, else normalized edit distance.
fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    if a == b {
        return 1.0;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    if a.contains(&b) || b.contains(&a) {
        return 0.6 + 0.4 * la.min(lb) as f64 / la.max(lb) as f64;
    }
    1.0 - levenshtein(&a, &b) as f64 / la.max(lb).max(1) as f64
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// A selector that finds `node` again in `tree` and keeps working across sessions: a unique id,
/// else role and label, with an index only when identical elements remain.
pub fn selector_for(node: &UiNode, tree: &UiTree) -> Selector {
    let mut sel = match (&node.id, &node.label) {
        (Some(id), _) if tree.iter().filter(|n| n.id.as_ref() == Some(id)).count() == 1 => {
            return Selector {
                id: Some(id.clone()),
                ..Selector::default()
            };
        }
        (_, Some(label)) => Selector {
            role: Some(node.role),
            text: Some(TextMatch::Exact(label.clone())),
            ..Selector::default()
        },
        (id, None) => Selector {
            role: Some(node.role),
            id: id.clone(),
            ..Selector::default()
        },
    };
    let matches = find_all(&sel, tree);
    if matches.len() > 1 {
        sel.index = matches.iter().position(|n| n.key == node.key);
    }
    sel
}

#[cfg(test)]
mod tests {
    use mdh_core::ui::{NodeFlags, RawNode, Rect};
    use mdh_observe::{RefTable, compress};

    use super::*;

    fn raw(
        class: &str,
        text: Option<&str>,
        id: Option<&str>,
        top: i32,
        clickable: bool,
    ) -> RawNode {
        RawNode {
            class: format!("android.widget.{class}"),
            text: text.map(Into::into),
            resource_id: id.map(|i| format!("com.example:id/{i}")),
            bounds: Rect::new(0, top, 1000, top + 100),
            flags: NodeFlags {
                enabled: true,
                clickable,
                ..NodeFlags::default()
            },
            ..RawNode::default()
        }
    }

    fn switch(label: &str, top: i32) -> RawNode {
        let mut n = raw("Switch", None, None, top, true);
        n.desc = Some(label.into());
        n.flags.checkable = true;
        n
    }

    fn tree(nodes: Vec<RawNode>) -> UiTree {
        let root = RawNode {
            class: "android.widget.FrameLayout".into(),
            bounds: Rect::new(0, 0, 1000, 2000),
            children: nodes,
            ..RawNode::default()
        };
        let mut tree = compress(&[root]);
        RefTable::default().assign(&mut tree);
        tree
    }

    fn label(t: &str) -> Target {
        Target::Selector(Selector {
            text: Some(TextMatch::Label(t.into())),
            ..Selector::default()
        })
    }

    fn resolved_label(target: &Target, tree: &UiTree) -> Option<String> {
        resolve(target, tree, None).ok()?.node?.label.clone()
    }

    #[test]
    fn parses_every_target_form() {
        assert_eq!(Target::parse("e12").unwrap(), Target::Ref("e12".into()));
        assert_eq!(
            Target::parse(" 100, 200 ").unwrap(),
            Target::Point { x: 100, y: 200 }
        );
        assert_eq!(
            Target::parse("role=switch;text=Wi-Fi").unwrap(),
            Target::Selector(Selector {
                role: Some(Role::Switch),
                text: Some(TextMatch::Exact("Wi-Fi".into())),
                ..Selector::default()
            })
        );
        assert_eq!(
            Target::parse("Network & internet").unwrap(),
            label("Network & internet")
        );
        // `=` alone doesn't make a selector; only known keys do.
        assert_eq!(Target::parse("a=b").unwrap(), label("a=b"));
        assert!(Target::parse("role=bogus").is_err());
        assert!(Target::parse("  ").is_err());
        assert!(Target::parse("e").is_ok_and(|t| t == label("e")));
    }

    #[test]
    fn selectors_round_trip_through_display() {
        for s in [
            "e3",
            "1,2",
            "role=switch;text=Wi-Fi",
            "id=login;index=1",
            "text~=net",
            "Sign in",
        ] {
            assert_eq!(Target::parse(s).unwrap().to_string(), s);
        }
    }

    #[test]
    fn label_prefers_exact_then_case_insensitive_then_contains() {
        let t = tree(vec![
            raw("Button", Some("Wi-Fi settings"), None, 0, true),
            raw("Button", Some("wi-fi"), None, 100, true),
        ]);
        assert_eq!(
            resolved_label(&label("wi-fi"), &t).as_deref(),
            Some("wi-fi")
        );
        assert_eq!(
            resolved_label(&label("WI-FI"), &t).as_deref(),
            Some("wi-fi")
        );
        assert_eq!(
            resolved_label(&label("settings"), &t).as_deref(),
            Some("Wi-Fi settings")
        );
    }

    #[test]
    fn prefers_the_control_over_text_with_the_same_label() {
        let t = tree(vec![
            raw("TextView", Some("Wi-Fi"), None, 0, false),
            switch("Wi-Fi", 100),
        ]);
        let r = resolve(&label("Wi-Fi"), &t, None).unwrap();
        assert_eq!(r.node.unwrap().role, Role::Switch);
    }

    #[test]
    fn ambiguity_and_absence_explain_themselves() {
        let t = tree(vec![
            raw("Button", Some("Delete"), None, 0, true),
            raw("Button", Some("Delete"), None, 100, true),
            raw("Button", Some("Network & internet"), None, 200, true),
        ]);
        match resolve(&label("Delete"), &t, None) {
            Err(Error::AmbiguousTarget { candidates, .. }) => assert_eq!(candidates.len(), 2),
            other => panic!("expected ambiguity, got {:?}", other.map(|r| r.point)),
        }
        match resolve(&label("Netwrk & internet"), &t, None) {
            Err(Error::ElementNotFound { candidates, .. }) => {
                assert_eq!(candidates, [r#"[e3] button "Network & internet""#]);
            }
            other => panic!("expected not found, got {:?}", other.map(|r| r.point)),
        }
    }

    #[test]
    fn selector_for_is_unique_and_minimal() {
        let t = tree(vec![
            raw("Button", Some("Delete"), None, 0, true),
            raw("Button", Some("Delete"), None, 100, true),
            raw("Button", Some("Save"), Some("save"), 200, true),
        ]);
        let second = t.find("e2").unwrap();
        let sel = selector_for(second, &t);
        assert_eq!(sel.to_string(), "role=button;text=Delete;index=1");
        assert_eq!(find_all(&sel, &t)[sel.index.unwrap()].r#ref, "e2");
        assert_eq!(
            selector_for(t.find("e3").unwrap(), &t).to_string(),
            "id=save"
        );
    }

    #[test]
    fn stale_ref_is_found_again_through_the_previous_tree() {
        // A shared ref table, like a session. The section title is part of the button's key, so
        // renaming the section gives the same button a new ref.
        let section = |title: &str| RawNode {
            class: "android.widget.LinearLayout".into(),
            desc: Some(title.into()),
            bounds: Rect::new(0, 0, 1000, 500),
            children: vec![raw("Button", Some("Save"), None, 100, true)],
            ..RawNode::default()
        };
        let mut refs = RefTable::default();
        let mut observe = |title: &str| {
            let root = RawNode {
                class: "android.widget.FrameLayout".into(),
                bounds: Rect::new(0, 0, 1000, 2000),
                children: vec![section(title)],
                ..RawNode::default()
            };
            let mut t = compress(&[root]);
            refs.assign(&mut t);
            t
        };
        let before = observe("Draft");
        let after = observe("Draft (edited)");
        let old_ref = before
            .iter()
            .find(|n| n.label.as_deref() == Some("Save"))
            .unwrap()
            .r#ref
            .clone();
        assert!(after.find(&old_ref).is_none(), "the old ref should be gone");

        let r = resolve(&Target::Ref(old_ref.clone()), &after, Some(&before)).unwrap();
        assert_eq!(r.node.unwrap().label.as_deref(), Some("Save"));
        assert_ne!(r.node.unwrap().r#ref, old_ref);
        assert!(matches!(
            resolve(&Target::Ref(old_ref), &after, None),
            Err(Error::ElementNotFound { .. })
        ));
    }
}
