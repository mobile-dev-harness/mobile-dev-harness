//! Rule checks on the compact tree (functional design F13.4): problems that need no baseline.
//!
//! Accessibility reports bounds clipped to what is visible, so content pushed off the screen can't
//! be told from content that ends there; that needs pixels or the views' own geometry.

use mdh_core::ui::{Rect, ScreenInfo};
use mdh_observe::{Role, UiNode, UiTree, render_line};
use serde::{Deserialize, Serialize};

/// Android's minimum touch target (Material, WCAG 2.5.8 target size).
const MIN_TARGET_DP: f64 = 48.0;
/// Two controls overlap when this share of the smaller one is covered by the other.
const OVERLAP_SHARE: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Controls at least 48 × 48 dp.
    TouchTarget,
    /// Controls have a label (text or content description) for screen readers.
    Label,
    /// Controls don't overlap each other.
    Overlap,
    /// Controls aren't drawn under the status or navigation bar.
    Obscured,
    /// Controls on one screen don't share a label (screen readers can't tell them apart).
    DuplicateLabel,
    /// Text stands out from its background (WCAG AA: 4.5:1, 3:1 for large text). Measured on the
    /// screenshot, see `pixels::contrast`.
    Contrast,
}

impl Rule {
    pub const ALL: [Rule; 6] = [
        Rule::TouchTarget,
        Rule::Label,
        Rule::Overlap,
        Rule::Obscured,
        Rule::DuplicateLabel,
        Rule::Contrast,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Rule::TouchTarget => "touch_target",
            Rule::Label => "label",
            Rule::Overlap => "overlap",
            Rule::Obscured => "obscured",
            Rule::DuplicateLabel => "duplicate_label",
            Rule::Contrast => "contrast",
        }
    }

    pub fn parse(s: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|r| r.name() == s.trim())
    }

    /// What the rule asks for, as shown in a verdict.
    pub fn describe(self) -> &'static str {
        match self {
            Rule::TouchTarget => "touch targets ≥ 48 dp",
            Rule::Label => "controls have labels",
            Rule::Overlap => "controls don't overlap",
            Rule::Obscured => "controls clear of the system bars",
            Rule::DuplicateLabel => "control labels are unique",
            Rule::Contrast => "text contrast ≥ 4.5:1",
        }
    }

    /// Duplicate labels are often deliberate (a "Delete" per row); everything else is a bug.
    pub fn advisory(self) -> bool {
        self == Rule::DuplicateLabel
    }

    /// Needs the screenshot rather than the tree.
    pub fn on_pixels(self) -> bool {
        self == Rule::Contrast
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: Rule,
    /// `[e5] button "OK" #ok: 36×36 dp, needs 48×48`.
    pub detail: String,
}

/// A node with what its ancestors say about it.
struct Placed<'a> {
    node: &'a UiNode,
    in_list: bool,
    /// The nearest scrolling ancestor's bounds: rows at its edges are clipped by it.
    viewport: Option<Rect>,
    /// Index path, to tell ancestors from other nodes.
    path: Vec<usize>,
}

fn placed(tree: &UiTree) -> Vec<Placed<'_>> {
    fn walk<'a>(
        nodes: &'a [UiNode],
        viewport: Option<Rect>,
        in_list: bool,
        path: &mut Vec<usize>,
        out: &mut Vec<Placed<'a>>,
    ) {
        for (i, n) in nodes.iter().enumerate() {
            path.push(i);
            out.push(Placed {
                node: n,
                in_list,
                viewport,
                path: path.clone(),
            });
            let inner = if n.state.scrollable {
                Some(n.bounds)
            } else {
                viewport
            };
            walk(
                &n.children,
                inner,
                in_list || n.role == Role::List,
                path,
                out,
            );
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(&tree.nodes, None, false, &mut Vec::new(), &mut out);
    out
}

fn interactive(n: &UiNode) -> bool {
    n.role.is_control()
}

fn inside(r: &Rect, screen: &Rect) -> bool {
    r.left >= screen.left
        && r.top >= screen.top
        && r.right <= screen.right
        && r.bottom <= screen.bottom
}

fn dp(px: i32, density: u32) -> f64 {
    f64::from(px) * 160.0 / f64::from(density.max(1))
}

/// Every violation of `rules` on the screen. `density` converts pixels to dp.
pub fn check(tree: &UiTree, screen: &ScreenInfo, density: u32, rules: &[Rule]) -> Vec<Violation> {
    let nodes = placed(tree);
    let controls: Vec<&Placed> = nodes.iter().filter(|p| interactive(p.node)).collect();
    let display = tree.screen;
    let mut out = Vec::new();
    let mut add = |rule: Rule, n: &UiNode, what: String| {
        out.push(Violation {
            rule,
            detail: format!("{}: {what}", render_line(n)),
        });
    };
    for rule in rules {
        match rule {
            Rule::TouchTarget => {
                for p in &controls {
                    let b = p.node.bounds;
                    // A control cut off by the screen or by its scrolling container can't be measured.
                    let clipped = p.viewport.is_some_and(|v| {
                        b.top <= v.top + 1
                            || b.bottom >= v.bottom - 1
                            || b.left <= v.left + 1
                            || b.right >= v.right - 1
                    });
                    if !inside(&b, &display) || (clipped && p.node.role == Role::Item) {
                        continue;
                    }
                    let (w, h) = (dp(b.width(), density), dp(b.height(), density));
                    if w < MIN_TARGET_DP - 0.5 || h < MIN_TARGET_DP - 0.5 {
                        add(*rule, p.node, format!("{w:.0}×{h:.0} dp, needs 48×48"));
                    }
                }
            }
            Rule::Label => {
                for p in &controls {
                    if p.node.label.as_deref().is_none_or(|l| l.trim().is_empty()) {
                        add(
                            *rule,
                            p.node,
                            "no label; screen readers can't name it".into(),
                        );
                    }
                }
            }
            Rule::Overlap => {
                for (i, a) in controls.iter().enumerate() {
                    for b in &controls[i + 1..] {
                        let related = b.path.starts_with(&a.path) || a.path.starts_with(&b.path);
                        if related {
                            continue;
                        }
                        let (ra, rb) = (a.node.bounds, b.node.bounds);
                        let Some(common) = ra.intersect(&rb) else {
                            continue;
                        };
                        let smaller = ra.area().min(rb.area()).max(1);
                        // One control drawn wholly inside another is a container and its content.
                        if common == ra || common == rb {
                            continue;
                        }
                        if common.area() as f64 / smaller as f64 >= OVERLAP_SHARE {
                            add(*rule, a.node, format!("overlaps {}", render_line(b.node)));
                        }
                    }
                }
            }
            Rule::Obscured => {
                // With the keyboard up, covered fields are normal; it goes away.
                if screen.keyboard {
                    continue;
                }
                for p in &controls {
                    if p.node.state.obscured {
                        add(
                            *rule,
                            p.node,
                            "mostly under the status or navigation bar".into(),
                        );
                    }
                }
            }
            Rule::DuplicateLabel => {
                for (i, a) in controls.iter().enumerate() {
                    let Some(label) = a.node.label.as_deref().map(str::to_lowercase) else {
                        continue;
                    };
                    if a.in_list {
                        continue;
                    }
                    let first = controls[..i].iter().all(|b| {
                        b.node.label.as_deref().map(str::to_lowercase).as_deref() != Some(&label)
                    });
                    let twins = controls[i + 1..]
                        .iter()
                        .filter(|b| {
                            !b.in_list
                                && b.node.label.as_deref().map(str::to_lowercase).as_deref()
                                    == Some(&label)
                        })
                        .count();
                    if first && twins > 0 {
                        let what = match twins {
                            1 => "another control has the same label".to_owned(),
                            n => format!("{n} other controls have the same label"),
                        };
                        add(*rule, a.node, what);
                    }
                }
            }
            Rule::Contrast => {}
        }
    }
    out
}
