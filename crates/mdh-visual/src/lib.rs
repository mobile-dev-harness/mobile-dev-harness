//! Visual: UI consistency checks, a check kind of the verification engine (ADR-0009).
//!
//! Scope (functional design F13; milestone M5): structural and pixel comparison against baselines
//! with masks for dynamic regions, comparison against design mocks (Figma), cross-configuration
//! layout problems (truncation, overlap, clipping), and rule checks on the tree and pixels (touch
//! target size, missing labels, contrast, design tokens). Implements `mdh_verify::Check`; findings
//! land in the run's verdict.
//!
//! So far: rule checks on the tree ([`rules`]) and structural baselines ([`baseline`]).

pub mod baseline;
pub mod rules;

use std::path::PathBuf;

use async_trait::async_trait;
use mdh_control::{Target, find_matches};
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
        let density = cx
            .session
            .control()
            .density()
            .await
            .unwrap_or(FALLBACK_DENSITY);
        let mut findings = Vec::new();

        let violations = rules::check(&observed.tree, &observed.screen, density, &rules);
        for rule in &rules {
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
                n => format!("{n} controls; {}", first.detail),
            };
            findings.push(finding(
                outcome,
                format!("ui: {}", rule.describe()),
                Some(observed),
                listed(
                    &mine
                        .iter()
                        .skip(1)
                        .map(|v| v.detail.clone())
                        .collect::<Vec<_>>(),
                ),
            ));
        }
        if !rules.is_empty() && violations.is_empty() {
            findings.push(finding(
                Outcome::Pass,
                format!("ui: {} rules hold", rules.len()),
                None,
                Vec::new(),
            ));
        }

        if config.baseline {
            let ignored = ignored(&config.ignore, &observed.tree)?;
            let snapshot = Snapshot::of(
                &observed.tree,
                observed.screen.activity.as_deref(),
                density,
                &ignored,
            );
            let screen = observed.tree.screen;
            let profile = format!("{}x{}-{density}dpi", screen.width(), screen.height());
            let scope = cx.scope.unwrap_or("screen");
            let store = Store {
                dir: self.baselines.clone(),
            };
            let path = store.path(scope, cx.checkpoint, &profile);
            let check = format!("ui matches baseline {scope}/{}", cx.checkpoint);
            match Store::load(&path)? {
                None => {
                    Store::write(&path, &snapshot)?;
                    findings.push(finding(
                        Outcome::Warn,
                        check,
                        Some(format!(
                            "no baseline yet; recorded {} as the baseline (commit it)",
                            path.display()
                        )),
                        Vec::new(),
                    ));
                }
                Some(base) => {
                    let deviations =
                        snapshot.deviations(&base, config.tolerance_dp.unwrap_or(TOLERANCE_DP));
                    let candidate = store.candidate(scope, cx.checkpoint, &profile);
                    if deviations.is_empty() {
                        let _ = std::fs::remove_file(&candidate);
                        findings.push(finding(Outcome::Pass, check, None, Vec::new()));
                    } else {
                        Store::write(&candidate, &snapshot)?;
                        let mut evidence = listed(&deviations[1..]);
                        evidence.push(format!(
                            "intended? `mdh visual approve {scope}` makes this the baseline"
                        ));
                        findings.push(finding(
                            Outcome::Fail,
                            check,
                            Some(format!(
                                "{} deviation{}: {}",
                                deviations.len(),
                                if deviations.len() == 1 { "" } else { "s" },
                                deviations[0]
                            )),
                            evidence,
                        ));
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
