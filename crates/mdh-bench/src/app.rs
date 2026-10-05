//! The apps tasks run on (`bench/apps.yaml`): where the source comes from and how to build it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    #[serde(skip)]
    pub name: String,
    /// A directory in this repository.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Or another repository at a pinned commit.
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    /// The application module and variant, when the build has several.
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    pub package: String,
    /// The build command, as the agent is told.
    pub build: String,
    /// Where the built APK lands, as the agent is told.
    pub apk: String,
    /// What the agent is told about the app.
    pub about: String,
}

pub fn load_all(repo: &Path) -> Result<BTreeMap<String, App>, String> {
    let path = repo.join("bench/apps.yaml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut apps: BTreeMap<String, App> =
        serde_norway::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for (name, app) in &mut apps {
        app.name.clone_from(name);
        match (&app.path, &app.repository, &app.commit) {
            (Some(_), None, None) | (None, Some(_), Some(_)) => {}
            _ => {
                return Err(format!(
                    "{}: app {name} needs `path`, or `repository` and `commit`",
                    path.display()
                ));
            }
        }
    }
    Ok(apps)
}

impl App {
    /// The source to copy from: the directory in this repository, or a checkout of the commit,
    /// made once (it downloads the repository at that commit).
    pub fn source(&self, repo: &Path) -> Result<PathBuf, String> {
        if let Some(p) = &self.path {
            return Ok(repo.join(p));
        }
        let (Some(url), Some(commit)) = (&self.repository, &self.commit) else {
            unreachable!("checked by load_all");
        };
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let dir = Path::new(&home).join(format!(
            ".cache/mdh-bench/{}-{}",
            self.name,
            &commit[..commit.len().min(12)]
        ));
        if dir.join(".mdh-bench-ready").is_file() {
            return Ok(dir);
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        eprintln!("fetching {url} at {commit} into {}", dir.display());
        for args in [
            vec!["init", "-q"],
            vec!["remote", "add", "origin", url],
            vec!["fetch", "-q", "--depth", "1", "origin", commit],
            vec!["checkout", "-q", "FETCH_HEAD"],
        ] {
            let ok = Command::new("git")
                .args(&args)
                .current_dir(&dir)
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                return Err(format!(
                    "git {} failed in {}",
                    args.join(" "),
                    dir.display()
                ));
            }
        }
        std::fs::write(dir.join(".mdh-bench-ready"), commit).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    /// `mdh run` arguments that pick the app's module and variant.
    pub fn mdh_args(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(m) = &self.module {
            out.extend(["--module".to_owned(), m.clone()]);
        }
        if let Some(v) = &self.variant {
            out.extend(["--variant".to_owned(), v.clone()]);
        }
        out
    }

    /// Flows every task on this app must keep passing.
    pub fn regression(&self, repo: &Path) -> Vec<PathBuf> {
        yaml_files(&repo.join("bench/apps").join(&self.name).join("regression"))
    }
}

pub fn yaml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.retain(|p| p.extension().is_some_and(|e| e == "yaml"));
    out.sort();
    out
}
