//! The text agents read: every section budgeted, folded lines counted.

use crate::index::Confidence;
use crate::model::DeclKind;
use crate::report::{Callers, ChangeKind, ImpactReport};

const CHANGES: usize = 12;
const EDGES: usize = 8;
const SCREENS: usize = 10;
const CALLER_DECLS: usize = 4;
const SITES: usize = 4;
const ITEMS: usize = 6;
const PATH: usize = 4;

pub fn render(r: &ImpactReport) -> String {
    let mut out = Vec::new();
    let decls = r.changes.len();
    if r.files_changed == 0 {
        return format!("no changes against {} ({})", r.base, r.base_commit);
    }
    out.push(format!(
        "impact vs {} ({}): {} file{} changed · {} declaration{}",
        r.base,
        r.base_commit,
        r.files_changed,
        plural(r.files_changed),
        decls,
        plural(decls)
    ));

    if !r.changes.is_empty() {
        out.push("changed".into());
        let width = r
            .changes
            .iter()
            .take(CHANGES)
            .map(|c| c.decl.chars().count())
            .max()
            .unwrap_or(0)
            .min(40);
        for c in r.changes.iter().take(CHANGES) {
            let (mark, what) = match c.change {
                ChangeKind::Added => ("+", "added".to_owned()),
                ChangeKind::Removed => ("-", "removed".to_owned()),
                ChangeKind::Signature => ("~", "signature".to_owned()),
                ChangeKind::Body => ("~", "body".to_owned()),
            };
            let what = match (&c.before, &c.after) {
                (Some(b), Some(a)) if c.kind == DeclKind::Resource => {
                    format!("value {} → {}", quote(b), quote(a))
                }
                (Some(b), Some(a)) => format!("{what} {b} → {a}"),
                (None, Some(a)) if c.change == ChangeKind::Added => format!("{what} {}", quote(a)),
                _ => what,
            };
            let file = c.file.rsplit('/').next().unwrap_or(&c.file);
            out.push(format!(
                "  {mark} {:<width$}  {what}  {file}:{}",
                c.decl, c.line
            ));
        }
        fold(&mut out, r.changes.len(), CHANGES, "changes");
    }
    if !r.cosmetic.is_empty() {
        out.push(format!(
            "  comments or formatting only: {}",
            short_list(&r.cosmetic, ITEMS, true)
        ));
    }

    if !r.edges.is_empty() {
        out.push("before → after".into());
        for e in r.edges.iter().take(EDGES) {
            let mut parts: Vec<String> = e
                .added
                .iter()
                .take(ITEMS)
                .map(|a| format!("+ {a}"))
                .collect();
            parts.extend(e.removed.iter().take(ITEMS).map(|a| format!("- {a}")));
            let more = (e.added.len() + e.removed.len()).saturating_sub(parts.len());
            if more > 0 {
                parts.push(format!("… {more} more"));
            }
            out.push(format!("  {}: {}", e.decl, parts.join(" · ")));
        }
        fold(&mut out, r.edges.len(), EDGES, "declarations");
    }

    if r.screens.is_empty() {
        if !r.changes.is_empty() {
            out.push("affected screens: none found".into());
        }
    } else {
        out.push("affected screens".into());
        let width = r
            .screens
            .iter()
            .take(SCREENS)
            .map(|s| s.screen.chars().count())
            .max()
            .unwrap_or(0);
        for s in r.screens.iter().take(SCREENS) {
            let mut line = format!("  {:<width$}  via {}", s.screen, path(&s.via));
            if s.changes > 1 {
                line.push_str(&format!(
                    " (+{} more change{})",
                    s.changes - 1,
                    plural(s.changes - 1)
                ));
            }
            if s.confidence != Confidence::Exact {
                line.push_str(&format!(" ({})", confidence(s.confidence)));
            }
            if let Some(h) = &s.host {
                line.push_str(&format!(" · in {h}"));
            }
            if !s.reach.is_empty() {
                line.push_str(&format!(" · reach: {}", s.reach.join(" | ")));
            }
            out.push(line);
        }
        fold(&mut out, r.screens.len(), SCREENS, "screens");
    }

    callers(&mut out, "callers of changed signatures", &r.callers);
    callers(&mut out, "still used after removal", &r.dangling);

    let v = &r.verify;
    let rows = [
        ("functional", &v.functional),
        ("ui", &v.ui),
        ("performance", &v.performance),
        ("compatibility", &v.compatibility),
        ("tests", &v.tests),
    ];
    if rows.iter().any(|(_, items)| !items.is_empty()) {
        out.push("verify".into());
        for (name, items) in rows {
            if items.is_empty() {
                continue;
            }
            if name == "functional" || name == "tests" {
                out.push(format!(
                    "  {name:<13}  {}",
                    short_list(items, ITEMS * 2, false)
                ));
            } else {
                for (i, item) in items.iter().take(ITEMS).enumerate() {
                    let label = if i == 0 { name } else { "" };
                    out.push(format!("  {label:<13}  {item}"));
                }
                if items.len() > ITEMS {
                    out.push(format!("  {:<13}  … {} more", "", items.len() - ITEMS));
                }
            }
        }
    }

    if !r.other_files.is_empty() {
        let build: Vec<String> = r
            .other_files
            .iter()
            .filter(|f| f.kind == "build")
            .map(|f| f.path.clone())
            .collect();
        let other: Vec<String> = r
            .other_files
            .iter()
            .filter(|f| f.kind != "build")
            .map(|f| f.path.clone())
            .collect();
        if !build.is_empty() {
            out.push(format!(
                "build configuration changed ({}): dependencies and build settings can affect every screen",
                short_list(&build, ITEMS, true)
            ));
        }
        if !other.is_empty() {
            out.push(format!("not analyzed: {}", short_list(&other, ITEMS, true)));
        }
    }
    for l in &r.limits {
        out.push(format!("note: {l}"));
    }
    out.join("\n")
}

fn callers(out: &mut Vec<String>, title: &str, groups: &[Callers]) {
    if groups.is_empty() {
        return;
    }
    out.push(title.into());
    for g in groups.iter().take(CALLER_DECLS) {
        let sites: Vec<String> = g
            .sites
            .iter()
            .take(SITES)
            .map(|s| {
                let file = s.file.rsplit('/').next().unwrap_or(&s.file);
                let mut t = format!("{file}:{} in {}", s.line, s.from);
                if let Some(n) = &s.note {
                    t.push_str(&format!(" — {n}"));
                }
                if s.confidence != Confidence::Exact {
                    t.push_str(&format!(" ({})", confidence(s.confidence)));
                }
                t
            })
            .collect();
        let more = g.sites.len().saturating_sub(SITES);
        let more = if more > 0 {
            format!(" · … {more} more")
        } else {
            String::new()
        };
        out.push(format!("  {}: {}{more}", g.decl, sites.join(" · ")));
    }
    fold(out, groups.len(), CALLER_DECLS, "declarations");
}

/// `a → b → c`, with the middle folded when long.
fn path(via: &[String]) -> String {
    if via.is_empty() {
        return "changed directly".into();
    }
    if via.len() <= PATH {
        return via.join(" → ");
    }
    format!("{} → … → {}", via[0], via[via.len() - 2..].join(" → "))
}

fn confidence(c: Confidence) -> &'static str {
    match c {
        Confidence::Exact => "exact",
        Confidence::Likely => "likely",
        Confidence::Ambiguous => "ambiguous: several declarations share the name",
    }
}

fn short_list(items: &[String], max: usize, file_names: bool) -> String {
    let shown: Vec<&str> = items
        .iter()
        .take(max)
        .map(|s| {
            if file_names {
                s.rsplit('/').next().unwrap_or(s)
            } else {
                s.as_str()
            }
        })
        .collect();
    let mut s = shown.join(", ");
    if items.len() > max {
        s.push_str(&format!(", … {} more", items.len() - max));
    }
    s
}

fn quote(s: &str) -> String {
    format!("\"{s}\"")
}

fn fold(out: &mut Vec<String>, total: usize, shown: usize, what: &str) {
    if total > shown {
        out.push(format!("  … {} more {what}", total - shown));
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
