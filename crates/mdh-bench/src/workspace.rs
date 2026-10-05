//! A fresh copy of the task's app per run, in its own git repository (without the app's history),
//! with the task applied.

use std::path::Path;
use std::process::Command;

use crate::task::{Edit, Kind, Task, apply};

/// What the copy leaves out: history, build output, Gradle and mdh state, saved flows and baselines
/// (no setup gets prior knowledge of the expected behavior).
const SKIP: &[&str] = &[
    ".git",
    "build",
    ".gradle",
    ".kotlin",
    ".mdh",
    ".idea",
    ".mdh-bench-ready",
    "local.properties",
];

/// Copies `source` to `dir` with the task's bug committed and its change left uncommitted; `fix`
/// (the reference fix or an alternative) is applied on top, to validate the checks.
pub fn prepare(
    source: &Path,
    dir: &Path,
    task: &Task,
    sdk: &Path,
    fix: Option<&[Edit]>,
) -> Result<(), String> {
    copy(source, dir).map_err(|e| format!("copying {}: {e}", source.display()))?;
    std::fs::write(
        dir.join("local.properties"),
        format!("sdk.dir={}\n", sdk.display()),
    )
    .map_err(|e| e.to_string())?;
    if task.kind == Kind::Fix {
        apply(dir, &task.bug)?;
    }
    git(dir, &["init", "-q", "-b", "main"])?;
    std::fs::write(dir.join(".git/info/exclude"), "local.properties\n")
        .map_err(|e| e.to_string())?;
    git(dir, &["add", "-A"])?;
    git(dir, &["commit", "-q", "-m", "Initial commit"])?;
    if task.kind == Kind::Verify {
        apply(dir, &task.change)?;
    }
    if let Some(edits) = fix {
        apply(dir, edits)?;
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|s| name == *s) {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=bench",
            "-c",
            "user.email=bench@localhost",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| e.to_string())?
        .success();
    ok.then_some(())
        .ok_or_else(|| format!("git {} failed", args.join(" ")))
}

/// The working tree's state: the diff against the commit and the untracked files, without build
/// output and tool state. Compared before and after a run to tell whether the agent edited code.
pub fn snapshot(dir: &Path) -> String {
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let untracked: Vec<String> = run(&["ls-files", "--others", "--exclude-standard"])
        .lines()
        .filter(|p| {
            !SKIP.iter().any(|s| p.starts_with(&format!("{s}/")))
                && !p.starts_with("app/build/")
                && !p.ends_with(".log")
        })
        .map(str::to_owned)
        .collect();
    format!("{}\n{}", run(&["diff"]), untracked.join("\n"))
}
