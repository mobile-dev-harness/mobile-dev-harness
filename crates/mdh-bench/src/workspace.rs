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

/// Build output and tool caches, wherever Gradle put them. The copy a run starts from has none
/// (see `SKIP`), so every such directory was made during the run.
const BUILT: &[&str] = &["build", ".gradle", ".kotlin"];

/// Removes build output from a kept workspace: a built copy of Now in Android is five times the
/// size of its sources, and a session keeps one per run.
pub fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // A link is never followed: only real directories under the workspace are touched.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        if BUILT.iter().any(|b| name == *b) {
            let _ = std::fs::remove_dir_all(entry.path());
        } else if name != ".git" {
            prune(&entry.path());
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_removes_build_output_and_keeps_sources() {
        let root = std::env::temp_dir().join(format!("mdh-bench-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "app/build/outputs",
            "app/src/main",
            ".gradle/8.0",
            "core/ui/build",
            ".git/build",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("app/src/main/Main.kt"), "").unwrap();
        std::fs::write(root.join("app/build.gradle.kts"), "").unwrap();
        prune(&root);
        assert!(!root.join("app/build").exists());
        assert!(!root.join(".gradle").exists());
        assert!(!root.join("core/ui/build").exists());
        assert!(root.join("core/ui").is_dir());
        assert!(root.join("app/src/main/Main.kt").is_file());
        assert!(root.join("app/build.gradle.kts").is_file());
        assert!(
            root.join(".git/build").is_dir(),
            "the repository is left alone"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
