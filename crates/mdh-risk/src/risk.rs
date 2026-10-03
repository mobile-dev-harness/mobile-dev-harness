//! Risk analysis (ADR-0011): the change's facts × the knowledge base → what could break where, and
//! what would show it. No device.

use std::collections::{BTreeMap, BTreeSet};

use mdh_impact::{ChangeKind, CompatFacts, DeclFacts, DeclKind, ImpactReport};
use serde::Serialize;

use crate::kb::{By, Shape, Triggers, kb};

/// Above every real API level: "this version or newer".
pub const NEWEST: u32 = 99;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Os,
    DeviceType,
    Vendor,
    ScreenSize,
}

impl Dimension {
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Os => "os version",
            Dimension::DeviceType => "device type",
            Dimension::Vendor => "vendor",
            Dimension::ScreenSize => "screen size",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Likelihood {
    High,
    Medium,
    Low,
}

/// Where a risk shows: a device requirement plus a configuration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Need {
    /// API levels the device may run, inclusive.
    pub api: (u32, u32),
    pub shape: Shape,
    /// Manufacturers, any of them; empty for any device.
    pub vendors: Vec<String>,
}

impl Need {
    pub fn any() -> Need {
        Need {
            api: (1, NEWEST),
            shape: Shape::Default,
            vendors: Vec::new(),
        }
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        match self.api {
            (1, NEWEST) => {}
            (lo, NEWEST) => parts.push(format!("API {lo}+")),
            (lo, hi) if lo == hi => parts.push(format!("API {lo}")),
            (1, hi) => parts.push(format!("API ≤{hi}")),
            (lo, hi) => parts.push(format!("API {lo}–{hi}")),
        }
        if self.shape != Shape::Default {
            parts.push(self.shape.describe());
        }
        if !self.vendors.is_empty() {
            parts.push(self.vendors.join("/"));
        }
        if parts.is_empty() {
            "any device".into()
        } else {
            parts.join(", ")
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Risk {
    /// `api-gate:33`, `behavior:pending-intent-mutability`, `screen-size`.
    pub id: String,
    pub dimension: Dimension,
    pub likelihood: Likelihood,
    /// One line: what could break.
    pub title: String,
    /// Why: declarations and what they matched, `file:line`.
    pub evidence: Vec<String>,
    /// Screens the change reaches; flows passing them verify the risk.
    pub screens: Vec<String>,
    /// Every one of these must hold; empty when it can't be verified here.
    pub needs: Vec<Need>,
    /// Compare state across a rotation.
    pub state_check: bool,
    /// Every flow exercises it (a build setting changed, not a screen).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub all_flows: bool,
    /// Why it can't be verified by mdh here, when it can't.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unverifiable: Option<String>,
    /// What to look at.
    pub verify: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The risks of the change, most likely first.
pub fn risks(report: &ImpactReport) -> Vec<Risk> {
    let facts = &report.compat;
    let mut out: BTreeMap<String, Risk> = BTreeMap::new();
    let all_screens: Vec<String> = report.screens.iter().map(|s| s.screen.clone()).collect();
    let min = facts.sdk.min.1.unwrap_or(1);
    let target = facts.sdk.target.1;
    let decls: Vec<&DeclFacts> = facts
        .decls
        .iter()
        .filter(|d| d.change != ChangeKind::Removed)
        .collect();

    sdk_levels(facts, &all_screens, &mut out);

    for d in &decls {
        // API-level branches: both sides of the boundary.
        for &level in &d.api_levels {
            if level <= min {
                continue;
            }
            add(
                &mut out,
                Risk {
                    id: format!("api-gate:{level}"),
                    dimension: Dimension::Os,
                    likelihood: Likelihood::High,
                    title: format!("behavior differs below and from API {level}"),
                    evidence: vec![format!("{} branches on API {level}", at(d))],
                    screens: d.screens.clone(),
                    needs: vec![
                        Need {
                            api: (min, level - 1),
                            ..Need::any()
                        },
                        Need {
                            api: (level, NEWEST),
                            ..Need::any()
                        },
                    ],
                    state_check: false,
                    all_flows: false,
                    unverifiable: None,
                    verify: format!(
                        "both branches: once on API {}–{} and once on API {level}+",
                        min,
                        level - 1
                    ),
                    source: None,
                },
            );
        }
        for b in &kb().behavior {
            if b.by == By::Target && target.is_some_and(|t| t < b.api) {
                continue;
            }
            let Some(matched) = matches(d, &b.triggers) else {
                continue;
            };
            let mut needs = vec![Need {
                api: (b.api.max(min), NEWEST),
                ..Need::any()
            }];
            if b.both_sides && min < b.api {
                needs.push(Need {
                    api: (min, b.api - 1),
                    ..Need::any()
                });
            }
            let when = match b.by {
                By::Device => format!("API {}+", b.api),
                By::Target => format!("API {}+, targetSdk {}", b.api, target.unwrap_or(b.api)),
            };
            add(
                &mut out,
                Risk {
                    id: format!("behavior:{}", b.id),
                    dimension: Dimension::Os,
                    likelihood: Likelihood::Medium,
                    title: format!("{} ({when})", b.summary),
                    evidence: vec![format!("{}: {matched}", at(d))],
                    screens: d.screens.clone(),
                    needs,
                    state_check: false,
                    all_flows: false,
                    unverifiable: None,
                    verify: b.verify.clone(),
                    source: Some(b.source.clone()),
                },
            );
        }
        for f in &kb().form_factors {
            let Some(matched) = matches(d, &f.triggers) else {
                continue;
            };
            let needs = f
                .cells
                .iter()
                .map(|&shape| Need {
                    shape,
                    ..Need::any()
                })
                .collect();
            add(
                &mut out,
                Risk {
                    id: format!("device:{}", f.id),
                    dimension: Dimension::DeviceType,
                    likelihood: Likelihood::Medium,
                    title: f.summary.clone(),
                    evidence: vec![format!("{}: {matched}", at(d))],
                    screens: d.screens.clone(),
                    needs,
                    state_check: f.state,
                    all_flows: false,
                    unverifiable: f.needs.clone().map(|n| format!("needs {n}")),
                    verify: f.verify.clone(),
                    source: Some(f.source.clone()),
                },
            );
        }
        for v in &kb().vendors {
            let Some(matched) = matches(d, &v.triggers) else {
                continue;
            };
            add(
                &mut out,
                Risk {
                    id: format!("vendor:{}", v.id),
                    dimension: Dimension::Vendor,
                    likelihood: Likelihood::Medium,
                    title: v.summary.clone(),
                    evidence: vec![format!("{}: {matched}", at(d))],
                    screens: d.screens.clone(),
                    needs: vec![Need {
                        vendors: v.vendors.clone(),
                        ..Need::any()
                    }],
                    state_check: false,
                    all_flows: false,
                    unverifiable: None,
                    verify: v.verify.clone(),
                    source: Some(v.source.clone()),
                },
            );
        }
        // Anything drawn on a screen can break at another size.
        // Qualified resources show only where their qualifier applies: the rules above.
        if is_visual(d) && !d.screens.is_empty() && d.qualifiers.is_empty() {
            let shapes: &[Shape] = if d.rtype.as_deref() == Some("string") {
                &[Shape::Compact]
            } else {
                &[Shape::Compact, Shape::Landscape, Shape::Tablet]
            };
            add(
                &mut out,
                Risk {
                    id: "screen-size".into(),
                    dimension: Dimension::ScreenSize,
                    likelihood: Likelihood::Medium,
                    title: "layout at other screen sizes and orientations".into(),
                    evidence: vec![format!("{} is drawn on screen", at(d))],
                    screens: d.screens.clone(),
                    needs: shapes
                        .iter()
                        .map(|&shape| Need {
                            shape,
                            ..Need::any()
                        })
                        .collect(),
                    state_check: false,
                    all_flows: false,
                    unverifiable: None,
                    verify: "no controls overlapping, cut off or under the system bars that the default size doesn't have".into(),
                    source: None,
                },
            );
        }
    }

    let mut risks: Vec<Risk> = out.into_values().collect();
    for r in &mut risks {
        r.needs.sort();
        r.needs.dedup();
        r.screens.sort();
        r.screens.dedup();
    }
    risks.sort_by(|a, b| {
        (a.likelihood, a.dimension, &a.id).cmp(&(b.likelihood, b.dimension, &b.id))
    });
    risks
}

/// `minSdk` lowered and `targetSdk` raised by the change.
fn sdk_levels(facts: &CompatFacts, screens: &[String], out: &mut BTreeMap<String, Risk>) {
    if let (Some(old), Some(new)) = facts.sdk.min
        && new < old
    {
        add(
            out,
            Risk {
                id: format!("min-sdk:{new}"),
                dimension: Dimension::Os,
                likelihood: Likelihood::High,
                title: format!(
                    "minSdk {old} → {new}: the app now runs on API {new}–{}",
                    old - 1
                ),
                evidence: vec![format!("minSdk {old} → {new} in the build script")],
                screens: screens.to_vec(),
                needs: vec![Need {
                    api: (new, old - 1),
                    ..Need::any()
                }],
                state_check: false,
                all_flows: true,
                unverifiable: None,
                verify: format!("the flows on API {new}"),
                source: None,
            },
        );
    }
    let (Some(old), Some(new)) = facts.sdk.target else {
        return;
    };
    if new <= old {
        return;
    }
    for b in kb().behavior.iter().filter(|b| b.by == By::Target) {
        if b.api <= old || b.api > new {
            continue;
        }
        let used: Vec<&String> = b
            .triggers
            .uses
            .iter()
            .filter(|u| facts.uses.contains(last_segment(u)))
            .collect();
        let manifest: Vec<&String> = b
            .triggers
            .manifest
            .iter()
            .filter(|m| m.starts_with('<'))
            .collect();
        let (likelihood, evidence) = if !used.is_empty() {
            let names: Vec<&str> = used.iter().map(|u| u.as_str()).collect();
            (
                Likelihood::High,
                format!("targetSdk {old} → {new}; the app uses {}", names.join(", ")),
            )
        } else if !manifest.is_empty() {
            (
                Likelihood::Medium,
                format!(
                    "targetSdk {old} → {new}; applies to manifest {}",
                    manifest
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        } else {
            (
                Likelihood::Low,
                format!("targetSdk {old} → {new}; nothing in the code uses it by name"),
            )
        };
        add(
            out,
            Risk {
                id: format!("behavior:{}", b.id),
                dimension: Dimension::Os,
                likelihood,
                title: format!("{} (API {}+, targetSdk {new})", b.summary, b.api),
                evidence: vec![evidence],
                screens: screens.to_vec(),
                needs: vec![Need {
                    api: (b.api, NEWEST),
                    ..Need::any()
                }],
                state_check: false,
                all_flows: true,
                unverifiable: None,
                verify: b.verify.clone(),
                source: Some(b.source.clone()),
            },
        );
    }
}

/// Merges a risk found again: more evidence, more screens, the higher likelihood.
fn add(out: &mut BTreeMap<String, Risk>, risk: Risk) {
    match out.get_mut(&risk.id) {
        Some(r) => {
            for e in risk.evidence {
                if !r.evidence.contains(&e) {
                    r.evidence.push(e);
                }
            }
            r.screens.extend(risk.screens);
            r.needs.extend(risk.needs);
            r.likelihood = r.likelihood.min(risk.likelihood);
            r.state_check |= risk.state_check;
            r.all_flows |= risk.all_flows;
        }
        None => {
            out.insert(risk.id.clone(), risk);
        }
    }
}

/// `Checkout.pay (app/src/…/Checkout.kt:42)`.
fn at(d: &DeclFacts) -> String {
    format!("{} ({}:{})", d.decl, d.file, d.line)
}

fn last_segment(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// What of `t` the declaration matches, or `None`.
fn matches(d: &DeclFacts, t: &Triggers) -> Option<String> {
    let own = last_segment(d.decl.split(' ').next_back().unwrap_or(&d.decl));
    let mut hits: BTreeSet<String> = BTreeSet::new();
    for u in &t.uses {
        let hit = if u.contains('.') {
            d.uses.iter().any(|x| x == u)
        } else {
            d.uses.iter().any(|x| x == u) || (d.kind != DeclKind::Manifest && own == u)
        };
        // Manifest entries are named by what they declare: `<uses-permission> POST_NOTIFICATIONS`.
        let manifest_name = d.kind == DeclKind::Manifest && own == u;
        if hit || manifest_name {
            hits.insert(u.clone());
        }
    }
    if d.kind == DeclKind::Manifest {
        let element = d.rtype.as_deref().unwrap_or_default();
        let changed = changed_attributes(d);
        for m in &t.manifest {
            if let Some(e) = m.strip_prefix('<').and_then(|m| m.strip_suffix('>')) {
                if e == element {
                    hits.insert(m.clone());
                }
            } else if changed.contains(m.as_str()) {
                hits.insert(format!("android:{m}"));
            }
        }
        if element == "uses-feature"
            && let Some(f) = t
                .features
                .iter()
                .find(|f| last_segment(f) == own || *f == own)
        {
            hits.insert(f.clone());
        }
    }
    // A layout's ids belong to the layout: the layout itself is the evidence.
    if !d.qualifiers.is_empty() && d.rtype.as_deref() != Some("id") {
        let quals: Vec<&str> = d.qualifiers.split('-').collect();
        for q in &t.qualifiers {
            if quals.iter().any(|x| x == q) {
                hits.insert(format!("{q} qualifier"));
            }
        }
    }
    (!hits.is_empty()).then(|| hits.into_iter().collect::<Vec<_>>().join(", "))
}

/// Manifest attributes added, removed or changed by the change.
fn changed_attributes(d: &DeclFacts) -> BTreeSet<&str> {
    fn parse(s: &Option<String>) -> BTreeMap<&str, &str> {
        s.as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|kv| kv.split_once('='))
            .collect()
    }
    let (before, after) = (parse(&d.before), parse(&d.after));
    if d.change == ChangeKind::Added {
        return after.keys().copied().collect();
    }
    before
        .keys()
        .chain(after.keys())
        .copied()
        .filter(|k| before.get(k) != after.get(k))
        .collect()
}

/// Layouts, drawn resources, composables and views.
fn is_visual(d: &DeclFacts) -> bool {
    match d.kind {
        DeclKind::Resource => matches!(
            d.rtype.as_deref(),
            Some("layout" | "dimen" | "style" | "drawable" | "menu" | "string")
        ),
        DeclKind::Function => d.annotations.iter().any(|a| a == "Composable"),
        DeclKind::Class => d
            .supertypes
            .iter()
            .any(|s| s.ends_with("View") || s.ends_with("Layout")),
        _ => false,
    }
}
