//! Tasks: a seeded bug (or a change to verify) in an app, the prompt, and the truth.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Judge an uncommitted change: the answer is a verdict, graded against `truth`.
    Verify,
    /// Fix a reported bug: graded by the hidden checks.
    Fix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Truth {
    Pass,
    Fail,
}

/// The least evidence that decides a task (bench/DESIGN.md, 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, serde::Serialize)]
pub enum Level {
    /// The code or the diff.
    L0,
    /// Running the app on one path.
    L1,
    /// Running it under a condition, or over several steps.
    L2,
    /// Measuring, or looking away from the change.
    L3,
}

/// Where a task comes from: results on re-injected real bugs are reported apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A real bug put back into the pinned commit, with its issue (`upstream`).
    Real,
    /// A bug written for the benchmark.
    Synthetic,
    /// A change from a real pull request (verify tasks).
    Pr,
}

/// A literal replacement in one file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub path: String,
    pub find: String,
    pub replace: String,
}

/// What a flow can't assert, read from the device right after one of the task's checks (the app
/// is left as that flow ended): the system bars' appearance, for one.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// The check (flow name) it follows.
    pub after: String,
    /// An `adb shell` command.
    pub shell: String,
    /// Texts its output must contain.
    #[serde(default)]
    pub contains: Vec<String>,
    /// Texts its output must not contain.
    #[serde(default)]
    pub lacks: Vec<String>,
    /// What that shows, for the verdict line.
    pub expect: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(skip)]
    pub id: String,
    #[serde(skip)]
    pub dir: PathBuf,
    pub kind: Kind,
    /// The app, from `bench/apps.yaml`.
    #[serde(default = "sample")]
    pub app: String,
    /// Version 1's tasks have none.
    #[serde(default)]
    pub level: Option<Level>,
    #[serde(default)]
    pub source: Option<Source>,
    /// The upstream issue a real bug comes from, `owner/repo#123`.
    #[serde(default)]
    pub upstream: Option<String>,
    /// Verify tasks: the other half of the pair (the same description, the opposite truth).
    #[serde(default)]
    pub pair: Option<String>,
    /// Kept out of the published task set (bench/DESIGN.md, 8).
    #[serde(default)]
    pub private: bool,
    /// What the task seeds, for the report.
    pub summary: String,
    /// Verify tasks: whether the change is correct.
    #[serde(default)]
    pub truth: Option<Truth>,
    /// Committed before the agent starts: the bug a fix task is about.
    #[serde(default)]
    pub bug: Vec<Edit>,
    /// Left uncommitted: the change a verify task is about.
    #[serde(default)]
    pub change: Vec<Edit>,
    /// The reference fix, to validate the hidden checks.
    #[serde(default)]
    pub fix: Vec<Edit>,
    /// Other correct fixes: the checks must accept them too.
    #[serde(default)]
    pub alternatives: Vec<Vec<Edit>>,
    /// Device observations after a check, for what flows can't assert.
    #[serde(default)]
    pub probes: Vec<Probe>,
    pub prompt: String,
}

fn sample() -> String {
    "sample".into()
}

impl Task {
    /// Hidden checks: mdh flows the grader replays after the agent is done (never shown to it).
    pub fn checks(&self) -> Vec<PathBuf> {
        crate::app::yaml_files(&self.dir.join("checks"))
    }
}

pub fn load_all(root: &Path, apps: &BTreeMap<String, App>) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("task.yaml").is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        let text = std::fs::read_to_string(dir.join("task.yaml")).map_err(|e| e.to_string())?;
        let mut t: Task = serde_norway::from_str(&text)
            .map_err(|e| format!("{}: {e}", dir.join("task.yaml").display()))?;
        t.id = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        t.dir = dir;
        match (t.kind, t.truth, t.bug.is_empty(), t.change.is_empty()) {
            (Kind::Verify, Some(_), _, false) | (Kind::Fix, None, false, _) => {}
            _ => {
                return Err(format!(
                    "{}: a verify task needs truth and change, a fix task bug",
                    t.id
                ));
            }
        }
        if t.checks().is_empty() {
            return Err(format!("{}: no checks/*.yaml", t.id));
        }
        if !apps.contains_key(&t.app) {
            return Err(format!("{}: no app {} in bench/apps.yaml", t.id, t.app));
        }
        let checks: Vec<String> = t
            .checks()
            .iter()
            .filter_map(|c| Some(c.file_stem()?.to_string_lossy().into_owned()))
            .collect();
        if let Some(p) = t.probes.iter().find(|p| !checks.contains(&p.after)) {
            return Err(format!(
                "{}: a probe follows `{}`, which is not one of its checks",
                t.id, p.after
            ));
        }
        tasks.push(t);
    }
    // A pair names each other and has opposite truths.
    for t in &tasks {
        let Some(other) = &t.pair else { continue };
        let ok = tasks.iter().any(|o| {
            &o.id == other
                && o.pair.as_deref() == Some(t.id.as_str())
                && o.kind == Kind::Verify
                && t.kind == Kind::Verify
                && o.truth != t.truth
        });
        if !ok {
            return Err(format!(
                "{}: pair {other} must be a verify task naming it back, with the opposite truth",
                t.id
            ));
        }
    }
    Ok(tasks)
}

/// Applies edits to the project at `root`; every `find` must occur exactly once.
pub fn apply(root: &Path, edits: &[Edit]) -> Result<(), String> {
    for e in edits {
        let p = root.join(&e.path);
        let text = std::fs::read_to_string(&p).map_err(|err| format!("{}: {err}", e.path))?;
        let n = text.matches(&e.find).count();
        if n != 1 {
            return Err(format!("{}: {:?} occurs {n} times", e.path, e.find));
        }
        std::fs::write(&p, text.replacen(&e.find, &e.replace, 1)).map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str, probe_after: &str) -> Result<Vec<Task>, String> {
        let root = std::env::temp_dir().join(format!("mdh-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let checks = root.join("bars/checks");
        std::fs::create_dir_all(&checks).unwrap();
        std::fs::write(
            checks.join("bars-dark.yaml"),
            "name: bars-dark\nsteps: []\n",
        )
        .unwrap();
        std::fs::write(
            root.join("bars/task.yaml"),
            format!(
                "kind: fix\nsummary: s\nprompt: p\nbug:\n- {{path: a, find: b, replace: c}}\n\
                 probes:\n- after: {probe_after}\n  shell: dumpsys window\n  lacks: [LIGHT_STATUS_BARS]\n  \
                 expect: light icons\n"
            ),
        )
        .unwrap();
        let apps =
            serde_norway::from_str("sample: {path: app, package: p, build: b, apk: a, about: x}\n")
                .unwrap();
        let tasks = load_all(&root, &apps);
        let _ = std::fs::remove_dir_all(&root);
        tasks
    }

    #[test]
    fn a_probe_follows_one_of_the_tasks_checks() {
        let tasks = load("probe", "bars-dark").unwrap();
        assert_eq!(tasks[0].probes[0].lacks, ["LIGHT_STATUS_BARS"]);
        assert!(tasks[0].probes[0].contains.is_empty());
        let err = load("probe-unknown", "bars-light").unwrap_err();
        assert!(err.contains("bars-light"), "{err}");
    }
}
