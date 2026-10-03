//! Tasks: a seeded bug (or a change to verify) in the sample app, the prompt, and the truth.

use std::path::{Path, PathBuf};

use serde::Deserialize;

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

/// A literal replacement in one file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub path: String,
    pub find: String,
    pub replace: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(skip)]
    pub id: String,
    #[serde(skip)]
    pub dir: PathBuf,
    pub kind: Kind,
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
    pub prompt: String,
}

impl Task {
    /// Hidden checks: mdh flows the grader replays after the agent is done (never shown to it).
    pub fn checks(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(self.dir.join("checks"))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        out.retain(|p| p.extension().is_some_and(|e| e == "yaml"));
        out.sort();
        out
    }
}

pub fn load_all(root: &Path) -> Result<Vec<Task>, String> {
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
        tasks.push(t);
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
