//! Structural baselines (functional design F13.1): the screen's elements and their geometry in dp,
//! stored per scope, checkpoint and device profile, compared element by element.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mdh_core::{Error, Result};
use mdh_observe::{UiNode, UiTree};
use serde::{Deserialize, Serialize};

/// Bump when the stored shape changes; older baselines are reported as needing a new recording.
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub activity: Option<String>,
    /// Screen size in dp.
    pub screen: (i32, i32),
    pub elements: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// What identifies the element across runs: role and id, else role and label, plus an
    /// occurrence index for repeats.
    pub identity: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Left, top, width, height in dp.
    pub bounds: [i32; 4],
}

impl Snapshot {
    /// The screen's elements, without those `ignored` (dynamic content such as clocks).
    pub fn of(
        tree: &UiTree,
        activity: Option<&str>,
        density: u32,
        ignored: &[&UiNode],
    ) -> Snapshot {
        let dp = |px: i32| (f64::from(px) * 160.0 / f64::from(density.max(1))).round() as i32;
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut elements = Vec::new();
        for n in tree.iter() {
            if ignored.iter().any(|i| std::ptr::eq(*i, n)) {
                continue;
            }
            let base = match (&n.id, &n.label) {
                (Some(id), _) => format!("{} #{id}", n.role.as_str()),
                (None, Some(label)) => format!("{} {label:?}", n.role.as_str()),
                (None, None) => n.role.as_str().to_owned(),
            };
            let count = seen.entry(base.clone()).or_default();
            let identity = if *count == 0 {
                base
            } else {
                format!("{base} [{}]", *count + 1)
            };
            *count += 1;
            let b = n.bounds;
            elements.push(Item {
                identity,
                role: n.role.as_str().to_owned(),
                label: n.label.clone(),
                detail: n.detail.clone(),
                id: n.id.clone(),
                bounds: [dp(b.left), dp(b.top), dp(b.width()), dp(b.height())],
            });
        }
        Snapshot {
            version: VERSION,
            activity: activity.map(str::to_owned),
            screen: (dp(tree.screen.width()), dp(tree.screen.height())),
            elements,
        }
    }

    /// What differs from `base`, one line per element: added, missing, moved, resized, text.
    pub fn deviations(&self, base: &Snapshot, tolerance_dp: i32) -> Vec<String> {
        let mut out = Vec::new();
        if base.activity != self.activity {
            out.push(format!(
                "screen {} → {}",
                base.activity.as_deref().unwrap_or("?"),
                self.activity.as_deref().unwrap_or("?")
            ));
        }
        let old: HashMap<&str, &Item> = base
            .elements
            .iter()
            .map(|e| (e.identity.as_str(), e))
            .collect();
        let new: HashMap<&str, &Item> = self
            .elements
            .iter()
            .map(|e| (e.identity.as_str(), e))
            .collect();
        for e in &self.elements {
            let Some(o) = old.get(e.identity.as_str()) else {
                out.push(format!("+ {} at {}", e.identity, at(&e.bounds)));
                continue;
            };
            let mut changes = Vec::new();
            if o.label != e.label {
                changes.push(format!(
                    "text {:?} → {:?}",
                    o.label.as_deref().unwrap_or(""),
                    e.label.as_deref().unwrap_or("")
                ));
            }
            if o.detail != e.detail {
                changes.push(format!(
                    "detail {:?} → {:?}",
                    o.detail.as_deref().unwrap_or(""),
                    e.detail.as_deref().unwrap_or("")
                ));
            }
            let [ox, oy, ow, oh] = o.bounds;
            let [x, y, w, h] = e.bounds;
            let far = |a: i32, b: i32| (a - b).abs() > tolerance_dp;
            if far(ox, x) || far(oy, y) {
                changes.push(format!("moved {}", direction(x - ox, y - oy)));
            }
            if far(ow, w) || far(oh, h) {
                changes.push(format!("resized {ow}×{oh} → {w}×{h} dp"));
            }
            if !changes.is_empty() {
                out.push(format!("~ {}: {}", e.identity, changes.join(", ")));
            }
        }
        for o in &base.elements {
            if !new.contains_key(o.identity.as_str()) {
                out.push(format!("- {} (was at {})", o.identity, at(&o.bounds)));
            }
        }
        out
    }
}

fn at(b: &[i32; 4]) -> String {
    format!("{},{} {}×{} dp", b[0], b[1], b[2], b[3])
}

/// `down 24 dp`, `left 8 dp and up 4 dp`.
fn direction(dx: i32, dy: i32) -> String {
    let mut parts = Vec::new();
    if dy != 0 {
        parts.push(format!(
            "{} {} dp",
            if dy > 0 { "down" } else { "up" },
            dy.abs()
        ));
    }
    if dx != 0 {
        parts.push(format!(
            "{} {} dp",
            if dx > 0 { "right" } else { "left" },
            dx.abs()
        ));
    }
    parts.join(" and ")
}

/// Baselines on disk: `<dir>/<scope>/<checkpoint>/<profile>.tree.json`, candidates next to them as
/// `<profile>.tree.new.json` until approved.
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn path(&self, scope: &str, checkpoint: &str, profile: &str) -> PathBuf {
        self.dir
            .join(scope)
            .join(checkpoint)
            .join(format!("{profile}.tree.json"))
    }

    pub fn candidate(&self, scope: &str, checkpoint: &str, profile: &str) -> PathBuf {
        self.dir
            .join(scope)
            .join(checkpoint)
            .join(format!("{profile}.tree.new.json"))
    }

    pub fn load(path: &Path) -> Result<Option<Snapshot>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice::<Snapshot>(&bytes)
                .ok()
                .filter(|s| s.version == VERSION)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write(path: &Path, snapshot: &Snapshot) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(snapshot).expect("snapshots are serializable");
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Promotes candidates to baselines (all of them, or those of one scope); returns the
    /// baselines written.
    pub fn approve(&self, scope: Option<&str>) -> Result<Vec<PathBuf>> {
        let root = match scope {
            Some(s) => self.dir.join(s),
            None => self.dir.clone(),
        };
        if !root.exists() {
            return Err(Error::InvalidFlow {
                flow: scope.unwrap_or("").to_owned(),
                reason: format!("no baselines under {}", root.display()),
            });
        }
        let mut approved = Vec::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)?.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(name) = p.file_name().and_then(|n| n.to_str())
                    && let Some(stem) = name.strip_suffix(".tree.new.json")
                {
                    let target = p.with_file_name(format!("{stem}.tree.json"));
                    std::fs::rename(&p, &target)?;
                    approved.push(target);
                }
            }
        }
        approved.sort();
        Ok(approved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(identity: &str, label: &str, bounds: [i32; 4]) -> Item {
        Item {
            identity: identity.into(),
            role: "button".into(),
            label: Some(label.into()),
            detail: None,
            id: None,
            bounds,
        }
    }

    fn snapshot(elements: Vec<Item>) -> Snapshot {
        Snapshot {
            version: VERSION,
            activity: Some("a/.Main".into()),
            screen: (400, 800),
            elements,
        }
    }

    #[test]
    fn reports_what_changed_beyond_the_tolerance() {
        let base = snapshot(vec![
            item("button #sign_in", "Sign in", [16, 400, 368, 48]),
            item("button #help", "Help", [16, 460, 368, 48]),
            item("text #title", "Welcome", [16, 100, 368, 32]),
        ]);
        let now = snapshot(vec![
            item("button #sign_in", "Log in", [16, 424, 368, 48]),
            item("text #title", "Welcome", [18, 101, 368, 32]),
            item("button #new", "New", [16, 520, 368, 96]),
        ]);
        assert_eq!(
            now.deviations(&base, 4),
            [
                r#"~ button #sign_in: text "Sign in" → "Log in", moved down 24 dp"#,
                "+ button #new at 16,520 368×96 dp",
                "- button #help (was at 16,460 368×48 dp)",
            ]
        );
        assert!(base.deviations(&base, 4).is_empty());
    }
}
