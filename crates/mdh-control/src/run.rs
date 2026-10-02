//! `run`: build, install, launch and observe in one step (functional design F2.6).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mdh_core::output::{Timings, millis};
use mdh_core::{Error, LaunchInfo, Result};
use mdh_project::{ApkSet, BuildOutcome, GradleProject, find_apks, render_build};
use serde::Serialize;

use crate::session::{Installed, Observation, Session};
use crate::settle::settle;
use crate::text::launch_text;

/// Run directories (build logs, verification evidence) kept in `.mdh/runs/`.
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
    /// Uninstall first: replaces an app signed with another key or a newer version, and clears
    /// its data.
    pub reinstall: bool,
}

/// What `run` did, step by step. When a step fails, the steps before it are still reported and
/// the error is in `failure`.
#[derive(Debug, Default, Serialize)]
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
    /// The failed step's error; reported through the output envelope, not serialized here.
    #[serde(skip)]
    pub failure: Option<Error>,
}

impl RunReport {
    /// Takes the error of the step that failed, if any (a failed build included).
    pub fn take_failure(&mut self) -> Option<Error> {
        self.failure.take()
    }

    fn push(&mut self, line: impl AsRef<str>) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(line.as_ref());
    }
}

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub package: String,
    pub apk: PathBuf,
    /// The ABI split installed, when the build has splits.
    pub abi: Option<String>,
    /// False when the same APK was already installed.
    pub installed: bool,
    pub duration_ms: u64,
}

impl Session {
    /// Builds the app, installs it if it changed, restarts it and reports its first settled
    /// screen. `on_task` receives Gradle's `> Task` lines as they happen. Errors before anything
    /// ran (no project, ambiguous variant, …) are returned as `Err`; later ones in the report.
    pub async fn run(
        &mut self,
        options: RunOptions,
        timings: &mut Timings,
        on_task: impl FnMut(&str),
    ) -> Result<RunReport> {
        let mut project = GradleProject::find(&options.project)?;
        if let Some(sdk) = self.control.sdk_root() {
            project = project.with_android_sdk(&sdk);
        }
        let mdh_dir = project.root().join(".mdh");
        let probing = Instant::now();
        let model = project.model(&mdh_dir.join("cache")).await?;
        timings.record("probe", probing);
        let (app, variant) = model.select(options.module.as_deref(), options.variant.as_deref())?;

        let mut report = RunReport::default();
        let apks = if options.build {
            let building = Instant::now();
            let log = new_run_dir(&mdh_dir.join("runs"), "build")?.join("build.log");
            let outcome = project.build(app, variant, &log, on_task).await?;
            timings.record("build", building);
            report.push(render_build(&outcome, project.root()));
            if !outcome.ok {
                report.failure = Some(Error::BuildFailed {
                    task: outcome.task.clone(),
                    errors: outcome.errors,
                });
                report.build = Some(outcome);
                return Ok(report);
            }
            let apks = outcome.apks.clone();
            report.build = Some(outcome);
            apks
        } else {
            find_apks(&app.dir, &variant.name)
        };

        let apks = apks.ok_or_else(|| Error::NoApk {
            variant: variant.name.clone(),
            reason: "no build output found".into(),
        });
        if let Err(e) = async { self.deploy(apks?, &options, &mut report, timings).await }.await {
            report.failure = Some(e);
        }
        Ok(report)
    }

    /// Install (when changed), restart, settle, observe; each step lands in `report` as it ends.
    async fn deploy(
        &mut self,
        apks: ApkSet,
        options: &RunOptions,
        report: &mut RunReport,
        timings: &mut Timings,
    ) -> Result<()> {
        let apk = apks.select(&self.control.abis().await?)?;
        let package = apk.application_id.clone();

        let installing = Instant::now();
        if options.reinstall {
            self.control.uninstall(&package).await?;
            self.state.installed.remove(&package);
        }
        let hash = file_hash(&apk.path)?;
        // Unchanged only if this exact APK is what we installed and nobody reinstalled since.
        let on_device = self.control.installed_path(&package).await?;
        let unchanged = matches!(
            (self.state.installed.get(&package), &on_device),
            (Some(last), Some(path)) if last.apk_hash == hash && last.device_path == *path
        );
        if !unchanged {
            if let Err(e) = self.control.install(&apk.path, options.grant).await {
                report.push(format!("install {package} → failed"));
                return Err(e);
            }
            if let Some(device_path) = self.control.installed_path(&package).await? {
                self.state.installed.insert(
                    package.clone(),
                    Installed {
                        apk_hash: hash,
                        device_path,
                    },
                );
            }
        }
        timings.record("install", installing);
        let abi = apk
            .abi
            .as_deref()
            .map(|a| format!(" [{a}]"))
            .unwrap_or_default();
        report.push(if unchanged {
            format!("install {package}{abi} → skipped, unchanged")
        } else {
            format!(
                "install {package}{abi} → ok ({:.1} s)",
                millis(installing) as f64 / 1000.0
            )
        });
        report.install = Some(InstallReport {
            package: package.clone(),
            apk: apk.path.clone(),
            abi: apk.abi.clone(),
            installed: !unchanged,
            duration_ms: millis(installing),
        });

        // A fresh start, like an IDE's Run, with the log cursor set first so a crash during
        // startup is reported.
        self.start_log_cursor().await?;
        let launching = Instant::now();
        self.control.stop(&package).await?;
        let launch = self.launch(&package).await?;
        timings.record("launch", launching);
        report.push(launch_text(&launch));
        report.launch = Some(launch);

        // Splash screens and first loads: wait until the app's first screen has settled.
        let settled = settle(&self.control, timings, None).await?;
        let entries = self.logs_since(self.state.log_cursor_ms).await?;
        let observation = self
            .observation_from(settled.snapshot, entries, false)
            .await?;
        report.push(&observation.text);
        report.observation = Some(observation);
        Ok(())
    }
}

fn file_hash(path: &Path) -> Result<u64> {
    let mut h = DefaultHasher::new();
    std::fs::read(path)?.hash(&mut h);
    Ok(h.finish())
}

/// `.mdh/runs/<unix ms>-<kind>/`, removing the oldest beyond `KEPT_RUNS`.
pub fn new_run_dir(runs: &Path, kind: &str) -> Result<PathBuf> {
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
    let dir = runs.join(format!("{now}-{kind}"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
