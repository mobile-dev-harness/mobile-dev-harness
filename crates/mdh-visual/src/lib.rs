//! Visual: UI consistency checks, a check kind of the verification engine (ADR-0009).
//!
//! Scope (functional design F13; milestone M5): structural and pixel comparison against baselines
//! with masks for dynamic regions, comparison against design mocks (Figma), cross-configuration
//! layout problems (truncation, overlap, clipping), and rule checks on the tree and pixels (touch
//! target size, missing labels, contrast, design tokens). Implements `mdh_verify::Check`; findings
//! land in the run's verdict.
//!
//! So far: rule checks on the tree ([`rules`]), structural baselines ([`baseline`]), pixel
//! baselines and contrast ([`pixels`]).

pub mod baseline;
pub mod pixels;
pub mod rules;

use std::path::PathBuf;

use async_trait::async_trait;
use mdh_control::{Target, find_matches};
use mdh_core::ui::Rect;
use mdh_core::{Error, Result};
use mdh_observe::UiNode;
use mdh_verify::{Check, CheckContext, CheckKind, Finding, Outcome};
use serde::Deserialize;

use crate::baseline::{Snapshot, Store};
use crate::rules::Rule;

/// Where baselines live, relative to the working directory (committed with the project).
pub const BASELINES_DIR: &str = ".mdh/baselines/visual";
/// Movement or size changes up to this many dp are rendering noise, not deviations.
const TOLERANCE_DP: i32 = 4;
/// Violations or deviations listed per finding.
const SHOWN: usize = 5;
/// Density assumed when the device can't tell (1 px = 1 dp).
const FALLBACK_DENSITY: u32 = 160;

/// The `visual:` section of a flow, or what `mdh visual check` asks for.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Rules that fail the verdict: a list of rule names, `all`, or `none`. Without it every rule
    /// still runs, as a warning.
    #[serde(default)]
    pub rules: Option<Rules>,
    /// Compare each checkpoint with its stored structural baseline.
    #[serde(default)]
    pub baseline: bool,
    /// Elements left out of baseline comparison (dynamic content: clocks, counters, feeds).
    #[serde(default)]
    pub ignore: Vec<String>,
    /// How far an element may move or resize, in dp, before it counts as a deviation.
    #[serde(default)]
    pub tolerance_dp: Option<i32>,
    /// With `baseline`, also compare pixels (default true).
    #[serde(default)]
    pub pixels: Option<bool>,
    /// Rectangles left out of the pixel comparison: `[left, top, width, height]` in dp.
    #[serde(default)]
    pub mask: Vec<[i32; 4]>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Rules {
    Keyword(String),
    List(Vec<String>),
}

impl Config {
    pub fn parse(value: Option<&serde_json::Value>) -> Result<Config> {
        match value {
            None => Ok(Config::default()),
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| Error::InvalidFlow {
                flow: "visual".into(),
                reason: e.to_string(),
            }),
        }
    }

    /// The rules to run and whether their violations fail the verdict.
    fn rules(&self) -> Result<(Vec<Rule>, bool)> {
        let parse_all = |names: &[String]| -> Result<Vec<Rule>> {
            names
                .iter()
                .map(|n| {
                    Rule::parse(n).ok_or_else(|| Error::InvalidFlow {
                        flow: "visual".into(),
                        reason: format!(
                            "unknown rule `{n}`; rules: {}",
                            Rule::ALL.map(Rule::name).join(", ")
                        ),
                    })
                })
                .collect()
        };
        match &self.rules {
            None => Ok((Rule::ALL.to_vec(), false)),
            Some(Rules::Keyword(k)) if k == "all" => Ok((Rule::ALL.to_vec(), true)),
            Some(Rules::Keyword(k)) if k == "none" => Ok((Vec::new(), true)),
            Some(Rules::Keyword(k)) => parse_all(std::slice::from_ref(k)).map(|r| (r, true)),
            Some(Rules::List(names)) => parse_all(names).map(|r| (r, true)),
        }
    }
}

/// The UI consistency check kind.
pub struct Visual {
    pub baselines: PathBuf,
}

impl Default for Visual {
    fn default() -> Self {
        Visual {
            baselines: PathBuf::from(BASELINES_DIR),
        }
    }
}

#[async_trait]
impl Check for Visual {
    fn kind(&self) -> CheckKind {
        CheckKind::Visual
    }

    async fn run(&self, cx: &mut CheckContext<'_>) -> Result<Vec<Finding>> {
        let config = Config::parse(cx.config)?;
        let (rules, strict) = config.rules()?;
        if rules.is_empty() && !config.baseline {
            return Ok(Vec::new());
        }
        let observed = cx.session.observe(false, cx.timings).await?;
        let control = cx.session.control();
        let density = control.density().await.unwrap_or(FALLBACK_DENSITY);
        let pixels_wanted = rules.iter().any(|r| r.on_pixels())
            || (config.baseline && config.pixels != Some(false));
        // One full-resolution screenshot serves contrast and the pixel baseline.
        let screenshot = if pixels_wanted {
            control
                .screenshot_png()
                .await
                .ok()
                .and_then(|png| pixels::decode(&png).ok())
        } else {
            None
        };
        let mut findings = Vec::new();

        let mut violations = rules::check(&observed.tree, &observed.screen, density, &rules);
        if rules.contains(&Rule::Contrast)
            && let Some(image) = &screenshot
        {
            violations.extend(pixels::contrast(&observed.tree, image, density));
        }
        for rule in &rules {
            if rule.on_pixels() && screenshot.is_none() {
                continue;
            }
            let mine: Vec<&rules::Violation> =
                violations.iter().filter(|v| v.rule == *rule).collect();
            let Some(first) = mine.first() else { continue };
            let outcome = if strict && !rule.advisory() {
                Outcome::Fail
            } else {
                Outcome::Warn
            };
            let observed = match mine.len() {
                1 => first.detail.clone(),
                n => format!("{n} elements; {}", first.detail),
            };
            let rest: Vec<String> = mine.iter().skip(1).map(|v| v.detail.clone()).collect();
            findings.push(finding(
                outcome,
                format!("ui: {}", rule.describe()),
                Some(observed),
                listed(&rest),
            ));
        }
        if !rules.is_empty() && violations.is_empty() {
            findings.push(finding(
                Outcome::Pass,
                match rules.len() {
                    1 => format!("ui: the {} rule holds", rules[0].name()),
                    n => format!("ui: {n} rules hold"),
                },
                None,
                Vec::new(),
            ));
        }

        if config.baseline {
            let ignored = ignored(&config.ignore, &observed.tree)?;
            let screen = observed.tree.screen;
            let profile = format!("{}x{}-{density}dpi", screen.width(), screen.height());
            let scope = cx.scope.unwrap_or("screen");
            let store = Store {
                dir: self.baselines.clone(),
            };
            let at = Baseline {
                store: &store,
                scope,
                checkpoint: cx.checkpoint,
                profile: &profile,
            };
            let snapshot = Snapshot::of(
                &observed.tree,
                observed.screen.activity.as_deref(),
                density,
                &ignored,
            );
            findings.push(at.structure(&snapshot, config.tolerance_dp.unwrap_or(TOLERANCE_DP))?);
            if config.pixels != Some(false) {
                match &screenshot {
                    None => findings.push(finding(
                        Outcome::Warn,
                        format!("ui pixels match baseline {scope}/{}", cx.checkpoint),
                        Some("no screenshot from this device; pixels not compared".into()),
                        Vec::new(),
                    )),
                    Some(image) => {
                        // Dynamic areas: system bars and keyboard, ignored elements, focused inputs
                        // (the cursor blinks), and rectangles given in dp.
                        let mut masks: Vec<Rect> = observed.screen.obstructions.clone();
                        masks.extend(ignored.iter().map(|n| n.bounds));
                        masks.extend(
                            observed
                                .tree
                                .iter()
                                .filter(|n| n.state.focused && n.role == mdh_observe::Role::Textbox)
                                .map(|n| n.bounds),
                        );
                        let px = |dp: i32| dp * density as i32 / 160;
                        masks.extend(
                            config.mask.iter().map(|[l, t, w, h]| {
                                Rect::new(px(*l), px(*t), px(l + w), px(t + h))
                            }),
                        );
                        findings.push(at.pixels(
                            image,
                            &masks,
                            &observed.tree,
                            density,
                            cx.run_dir,
                        )?);
                    }
                }
            }
        }

        // A picture of what failed, next to the verdict's own evidence.
        if findings.iter().any(|f| f.outcome == Outcome::Fail)
            && let Some(dir) = cx.run_dir
            && let Ok(image) = cx.session.control().capture(1024, cx.timings).await
        {
            let name = format!("visual-{}.jpg", cx.checkpoint);
            if std::fs::write(dir.join(&name), &image.bytes).is_ok()
                && let Some(f) = findings.iter_mut().find(|f| f.outcome == Outcome::Fail)
            {
                f.evidence.push(format!("screenshot: {name}"));
            }
        }
        for f in &mut findings {
            f.step = cx.step;
        }
        Ok(findings)
    }
}

/// One checkpoint's baselines.
struct Baseline<'a> {
    store: &'a Store,
    scope: &'a str,
    checkpoint: &'a str,
    profile: &'a str,
}

impl Baseline<'_> {
    fn approve_hint(&self) -> String {
        format!(
            "intended? `mdh visual approve {}` makes this the baseline",
            self.scope
        )
    }

    /// Elements compared with the structural baseline; the first run records it.
    fn structure(&self, snapshot: &Snapshot, tolerance_dp: i32) -> Result<Finding> {
        let path = self.store.path(self.scope, self.checkpoint, self.profile);
        let check = format!("ui matches baseline {}/{}", self.scope, self.checkpoint);
        let Some(base) = Store::load(&path)? else {
            Store::write(&path, snapshot)?;
            return Ok(finding(
                Outcome::Warn,
                check,
                Some(format!(
                    "no baseline yet; recorded {} as the baseline (commit it)",
                    path.display()
                )),
                Vec::new(),
            ));
        };
        let deviations = snapshot.deviations(&base, tolerance_dp);
        let candidate = self
            .store
            .candidate(self.scope, self.checkpoint, self.profile);
        if deviations.is_empty() {
            let _ = std::fs::remove_file(&candidate);
            return Ok(finding(Outcome::Pass, check, None, Vec::new()));
        }
        Store::write(&candidate, snapshot)?;
        let mut evidence = listed(&deviations[1..]);
        evidence.push(self.approve_hint());
        Ok(finding(
            Outcome::Fail,
            check,
            Some(format!(
                "{} deviation{}: {}",
                deviations.len(),
                plural(deviations.len()),
                deviations[0]
            )),
            evidence,
        ))
    }

    /// The screenshot compared with the pixel baseline, changed regions named by the elements they
    /// fall in; a diff image goes to the run directory.
    fn pixels(
        &self,
        image: &image::RgbImage,
        masks: &[Rect],
        tree: &mdh_observe::UiTree,
        density: u32,
        run_dir: Option<&std::path::Path>,
    ) -> Result<Finding> {
        let frame = pixels::frame(image);
        let path = self.store.image(self.scope, self.checkpoint, self.profile);
        let check = format!(
            "ui pixels match baseline {}/{}",
            self.scope, self.checkpoint
        );
        let Some(base) = pixels::load(&path)? else {
            pixels::save(&path, &frame)?;
            return Ok(finding(
                Outcome::Warn,
                check,
                Some(format!(
                    "no pixel baseline yet; recorded {} (commit it)",
                    path.display()
                )),
                Vec::new(),
            ));
        };
        let candidate = self
            .store
            .image_candidate(self.scope, self.checkpoint, self.profile);
        let comparison = pixels::compare(&base, &frame, masks);
        if comparison.size_mismatch {
            pixels::save(&candidate, &frame)?;
            return Ok(finding(
                Outcome::Fail,
                check,
                Some(format!(
                    "the screenshot is {}×{}, the baseline {}×{}",
                    frame.width(),
                    frame.height(),
                    base.width(),
                    base.height()
                )),
                vec![self.approve_hint()],
            ));
        }
        if comparison.regions.is_empty() {
            let _ = std::fs::remove_file(&candidate);
            return Ok(finding(Outcome::Pass, check, None, Vec::new()));
        }
        pixels::save(&candidate, &frame)?;
        let dp = |px: i32| px * 160 / density.max(1) as i32;
        let describe = |r: &pixels::Region| {
            let where_ = pixels::element_at(tree, &r.rect)
                .map(|n| format!(" in {}", mdh_observe::render_line(n)))
                .unwrap_or_default();
            format!(
                "{}×{} dp at {},{} ({:.0}% of its pixels){where_}",
                dp(r.rect.width()),
                dp(r.rect.height()),
                dp(r.rect.left),
                dp(r.rect.top),
                r.changed * 100.0
            )
        };
        let lines: Vec<String> = comparison.regions.iter().map(describe).collect();
        let mut evidence = listed(&lines[1..]);
        if let Some(dir) = run_dir {
            let name = format!("visual-{}-diff.png", self.checkpoint);
            if pixels::save(
                &dir.join(&name),
                &pixels::diff_image(&frame, &comparison.regions),
            )
            .is_ok()
            {
                evidence.push(format!("diff: {name}"));
            }
        }
        evidence.push(self.approve_hint());
        Ok(finding(
            Outcome::Fail,
            check,
            Some(format!(
                "{} region{} changed: {}",
                lines.len(),
                plural(lines.len()),
                lines[0]
            )),
            evidence,
        ))
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Promotes candidate baselines (all, or one scope's) after deviations were reviewed.
pub fn approve(scope: Option<&str>) -> Result<String> {
    let store = Store {
        dir: PathBuf::from(BASELINES_DIR),
    };
    let approved = store.approve(scope)?;
    Ok(if approved.is_empty() {
        "no candidate baselines to approve".to_owned()
    } else {
        let lines: Vec<String> = approved
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect();
        format!(
            "approved {} baseline{}:\n{}",
            approved.len(),
            if approved.len() == 1 { "" } else { "s" },
            lines.join("\n")
        )
    })
}

fn ignored<'t>(targets: &[String], tree: &'t mdh_observe::UiTree) -> Result<Vec<&'t UiNode>> {
    let mut out = Vec::new();
    for t in targets {
        out.extend(find_matches(&Target::parse(t)?, tree));
    }
    Ok(out)
}

fn listed(items: &[String]) -> Vec<String> {
    let mut out: Vec<String> = items.iter().take(SHOWN - 1).cloned().collect();
    if items.len() > SHOWN - 1 {
        out.push(format!("… {} more", items.len() - (SHOWN - 1)));
    }
    out
}

fn finding(
    outcome: Outcome,
    check: String,
    observed: Option<String>,
    evidence: Vec<String>,
) -> Finding {
    Finding {
        kind: CheckKind::Visual,
        outcome,
        check,
        observed,
        step: None,
        evidence,
    }
}
