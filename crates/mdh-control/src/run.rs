//! `run`: build, install, launch and observe in one step (functional design F2.6).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mdh_core::output::{Timings, millis};
use mdh_core::{Error, LaunchInfo, Result};
use mdh_project::{BuildOutcome, GradleProject, find_apk, render_build};
use serde::Serialize;

use crate::session::{Observation, Session};
use crate::settle::settle;
use crate::text::launch_text;

/// Build logs kept in `.mdh/runs/`.
const KEPT_RUNS: usize = 20;

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Any directory inside the Gradle build.
    pub project: PathBuf,
    /// Gradle path such as `:app`; needed when the build has several application modules.
    pub module: Option<String>,
    /// Such as `debug` or `freeDebug`; defaults to the debug variant.
    pub variant: Option<String>,
    /// Build before installing; when false, the last built APK is used.
    pub build: bool,
    /// Grant all runtime permissions on install.
    pub grant: bool,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch: Option<LaunchInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<Observation>,
    /// The compact text form agents read.
    pub text: String,
}

impl RunReport {
    /// Set when the build failed; the report then stops after the build.
    pub fn build_error(&self) -> Option<Error> {
        let b = self.build.as_ref()?;
        (!b.ok).then(|| Error::BuildFailed {
            task: b.task.clone(),
            errors: b.errors,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub package: String,
    pub apk: PathBuf,
    /// False when the same APK was already installed.
    pub installed: bool,
    pub duration_ms: u64,
}

impl Session {
    /// Builds the app, installs it if it changed, restarts it and reports its first settled
    /// screen. `on_task` receives Gradle's `> Task` lines as they happen.
    pub async fn run(
        &mut self,
        options: RunOptions,
        timings: &mut Timings,
        on_task: impl FnMut(&str),
    ) -> Result<RunReport> {
        let project = GradleProject::find(&options.project)?;
        let mdh_dir = project.root().join(".mdh");
        let probing = Instant::now();
        let model = project.model(&mdh_dir.join("cache")).await?;
        timings.record("probe", probing);
        let (app, variant) = model.select(options.module.as_deref(), options.variant.as_deref())?;

        let mut text = Vec::new();
        let (build, apk) = if options.build {
            let building = Instant::now();
            let log = new_run_dir(&mdh_dir.join("runs"))?.join("build.log");
            let outcome = project.build(app, variant, &log, on_task).await?;
            timings.record("build", building);
            text.push(render_build(&outcome, project.root()));
            if !outcome.ok {
                return Ok(RunReport {
                    build: Some(outcome),
                    install: None,
                    launch: None,
                    observation: None,
                    text: text.join("\n"),
                });
            }
            let apk = outcome.apk.clone();
            (Some(outcome), apk)
        } else {
            (None, find_apk(&app.dir, &variant.name))
        };
        let apk = apk.ok_or_else(|| Error::NoApk {
            variant: variant.name.clone(),
        })?;

        let installing = Instant::now();
        let package = apk.application_id.clone();
        let hash = file_hash(&apk.path)?;
        let unchanged = self.state.installed.get(&package) == Some(&hash)
            && self.control.is_installed(&package).await?;
        if !unchanged {
            self.control.install(&apk.path, options.grant).await?;
            self.state.installed.insert(package.clone(), hash);
        }
        timings.record("install", installing);
        text.push(if unchanged {
            format!("install {package} → skipped, unchanged")
        } else {
            format!(
                "install {package} → ok ({:.1} s)",
                millis(installing) as f64 / 1000.0
            )
        });
        let install = InstallReport {
            package: package.clone(),
            apk: apk.path.clone(),
            installed: !unchanged,
            duration_ms: millis(installing),
        };

        // A fresh start, like an IDE's Run, with the log cursor set first so a crash during
        // startup is reported.
        self.start_log_cursor().await?;
        let launching = Instant::now();
        self.control.stop(&package).await?;
        let launch = self.launch(&package).await?;
        timings.record("launch", launching);
        text.push(launch_text(&launch));

        // Splash screens and first loads: wait until the app's first screen has settled.
        let settled = settle(&self.control, timings, None).await?;
        let entries = self.logs_since(self.state.log_cursor_ms).await?;
        let observation = self
            .observation_from(settled.snapshot, entries, false)
            .await?;
        text.push(observation.text.clone());

        Ok(RunReport {
            build,
            install: Some(install),
            launch: Some(launch),
            observation: Some(observation),
            text: text.join("\n"),
        })
    }
}

fn file_hash(path: &Path) -> Result<u64> {
    let mut h = DefaultHasher::new();
    std::fs::read(path)?.hash(&mut h);
    Ok(h.finish())
}

/// `.mdh/runs/<unix ms>-build/`, removing the oldest beyond `KEPT_RUNS`.
fn new_run_dir(runs: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(runs)?;
    let mut existing: Vec<PathBuf> = std::fs::read_dir(runs)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    existing.sort();
    let excess = (existing.len() + 1).saturating_sub(KEPT_RUNS);
    for old in existing.iter().take(excess) {
        let _ = std::fs::remove_dir_all(old);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let dir = runs.join(format!("{now}-build"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
