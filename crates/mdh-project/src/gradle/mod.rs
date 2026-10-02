//! Gradle projects: finding them, probing their Android application modules, building, and
//! locating the built APK.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use mdh_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::diagnostics::{self, Diagnostic, Severity};

const PROBE_SCRIPT: &str = include_str!("probe.gradle");
const PROBE_MARKER: &str = "MDH_PROBE ";
const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const SKIPPED_DIRS: &[&str] = &["build", ".gradle", ".git", ".idea", ".mdh", "node_modules"];

/// A Gradle build rooted at the directory holding `settings.gradle(.kts)`.
#[derive(Debug, Clone)]
pub struct GradleProject {
    root: PathBuf,
    gradle: PathBuf,
    env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectModel {
    pub apps: Vec<AppModule>,
}

/// A module applying `com.android.application`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppModule {
    /// Gradle path, `:` for a single-module build, `:app` otherwise.
    pub path: String,
    pub dir: PathBuf,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    #[serde(rename = "applicationId")]
    pub application_id: Option<String>,
    #[serde(rename = "buildType")]
    pub build_type: Option<String>,
}

/// The APKs AGP wrote for one variant: a single one, or one per ABI split plus maybe a
/// universal one.
#[derive(Debug, Clone, Serialize)]
pub struct ApkSet {
    pub application_id: String,
    pub outputs: Vec<ApkOutput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApkOutput {
    pub path: PathBuf,
    /// Empty for a single or universal APK.
    pub abis: Vec<String>,
    pub version_name: Option<String>,
}

/// The APK chosen for a device.
#[derive(Debug, Clone, Serialize)]
pub struct Apk {
    pub path: PathBuf,
    pub application_id: String,
    pub version_name: Option<String>,
    pub abi: Option<String>,
}

impl ApkSet {
    /// The APK for a device supporting `device_abis` (preferred first): its ABI split, else a
    /// universal APK. An empty list (unknown device) takes the universal or first APK.
    pub fn select(&self, device_abis: &[String]) -> Result<Apk> {
        let pick = |o: &ApkOutput, abi: Option<&String>| Apk {
            path: o.path.clone(),
            application_id: self.application_id.clone(),
            version_name: o.version_name.clone(),
            abi: abi.cloned(),
        };
        for abi in device_abis {
            if let Some(o) = self.outputs.iter().find(|o| o.abis.contains(abi)) {
                return Ok(pick(o, Some(abi)));
            }
        }
        if let Some(o) = self.outputs.iter().find(|o| o.abis.is_empty()) {
            return Ok(pick(o, None));
        }
        if device_abis.is_empty()
            && let Some(o) = self.outputs.first()
        {
            return Ok(pick(o, None));
        }
        let built: Vec<String> = self.outputs.iter().flat_map(|o| o.abis.clone()).collect();
        Err(Error::NoApk {
            variant: self.application_id.clone(),
            reason: format!(
                "the device supports {} but the build only has {}; add its ABI or enable a universal APK",
                device_abis.join(", "),
                built.join(", ")
            ),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildOutcome {
    pub task: String,
    pub ok: bool,
    pub duration_ms: u64,
    /// Every task was up to date: nothing changed since the last build.
    pub up_to_date: bool,
    pub apks: Option<ApkSet>,
    pub errors: usize,
    pub warnings: usize,
    /// Errors first, in order of appearance; paths relative to the project root.
    pub diagnostics: Vec<Diagnostic>,
    pub log: PathBuf,
}

impl GradleProject {
    /// Finds the build containing `dir`, using its wrapper when there is one.
    pub fn find(dir: &Path) -> Result<Self> {
        let dir = dir.canonicalize().map_err(|_| Error::ProjectNotFound {
            dir: dir.display().to_string(),
        })?;
        let root = dir
            .ancestors()
            .find(|d| {
                d.join("settings.gradle.kts").is_file() || d.join("settings.gradle").is_file()
            })
            .ok_or_else(|| Error::ProjectNotFound {
                dir: dir.display().to_string(),
            })?
            .to_path_buf();
        let wrapper = root.join(if cfg!(windows) {
            "gradlew.bat"
        } else {
            "gradlew"
        });
        let gradle = if wrapper.is_file() {
            wrapper
        } else {
            PathBuf::from("gradle")
        };
        Ok(Self {
            root,
            gradle,
            env: Vec::new(),
        })
    }

    /// Points Gradle at `sdk` when the environment doesn't name one, so a missing `local.properties`
    /// doesn't fail the build (`sdk.dir` there still wins).
    pub fn with_android_sdk(mut self, sdk: &Path) -> Self {
        if std::env::var_os("ANDROID_HOME").is_none()
            && std::env::var_os("ANDROID_SDK_ROOT").is_none()
        {
            self.env
                .push(("ANDROID_HOME".into(), sdk.display().to_string()));
        }
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The application modules and variants, from `cache_dir` when no build file changed.
    pub async fn model(&self, cache_dir: &Path) -> Result<ProjectModel> {
        let key = self.build_files_hash();
        let cache = cache_dir.join("gradle-model.json");
        if let Some(model) = std::fs::read(&cache)
            .ok()
            .and_then(|b| serde_json::from_slice::<CachedModel>(&b).ok())
            .filter(|c| c.key == key)
            .map(|c| c.model)
        {
            return Ok(model);
        }

        std::fs::create_dir_all(cache_dir)?;
        let script = cache_dir.join("mdh-probe.gradle");
        std::fs::write(&script, PROBE_SCRIPT)?;
        let script = script.to_string_lossy().into_owned();
        // Without these, configuration cache or configure-on-demand would skip the init script's
        // hooks for some modules.
        let args = [
            "-q",
            "--init-script",
            &script,
            "--no-configuration-cache",
            "--no-configure-on-demand",
            "mdhProbe",
        ];
        let output = Command::new(&self.gradle)
            .args(args)
            .envs(self.env.iter().cloned())
            .current_dir(&self.root)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| gradle_missing(&self.gradle, e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some(json) = stdout.lines().find_map(|l| l.strip_prefix(PROBE_MARKER)) else {
            let all = format!("{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
            let message = diagnostics::parse(&all).into_iter().next().map_or_else(
                || "Gradle printed no project model".to_owned(),
                |d| d.message,
            );
            return Err(Error::ProbeFailed { message });
        };
        let model: ProjectModel = serde_json::from_str(json).map_err(|e| Error::Parse {
            tool: "mdh Gradle probe".into(),
            detail: e.to_string(),
        })?;
        let cached = CachedModel { key, model };
        std::fs::write(&cache, serde_json::to_vec(&cached).expect("serializable"))?;
        Ok(cached.model)
    }

    /// Runs `assemble<Variant>` for `app`, streaming the output to `log` and every `> Task` line
    /// to `on_task`.
    pub async fn build(
        &self,
        app: &AppModule,
        variant: &Variant,
        log: &Path,
        mut on_task: impl FnMut(&str),
    ) -> Result<BuildOutcome> {
        let task = assemble_task(app, variant);
        let started = Instant::now();
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut log_file = tokio::fs::File::create(log).await?;
        let mut child = Command::new(&self.gradle)
            .args([task.as_str(), "--console=plain"])
            .envs(self.env.iter().cloned())
            .current_dir(&self.root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| gradle_missing(&self.gradle, e))?;

        // Interleave stdout and stderr line by line, as Gradle prints them.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut output = String::new();
        let collect = async {
            while let Some(line) = rx.recv().await {
                if let Some(t) = line.strip_prefix("> Task ") {
                    on_task(t);
                }
                log_file.write_all(line.as_bytes()).await?;
                log_file.write_all(b"\n").await?;
                output.push_str(&line);
                output.push('\n');
            }
            child.wait().await
        };
        let status = tokio::time::timeout(BUILD_TIMEOUT, collect)
            .await
            .map_err(|_| Error::CommandFailed {
                command: format!("{} {task}", self.gradle.display()),
                code: None,
                stderr: format!("timed out after {} minutes", BUILD_TIMEOUT.as_secs() / 60),
            })??;
        log_file.flush().await?;

        let mut diagnostics = diagnostics::parse(&output);
        self.add_context(&mut diagnostics, &app.dir);
        diagnostics.sort_by_key(|d| d.severity != Severity::Error);
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let ok = status.success();
        Ok(BuildOutcome {
            apks: if ok {
                find_apks(&app.dir, &variant.name)
            } else {
                None
            },
            up_to_date: ok && all_up_to_date(&output),
            ok,
            errors,
            warnings: diagnostics.len() - errors,
            diagnostics,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            log: log.to_path_buf(),
            task,
        })
    }

    /// Paths relative to the root, and the offending line for diagnostics that don't print it.
    fn add_context(&self, diagnostics: &mut [Diagnostic], module_dir: &Path) {
        for d in diagnostics {
            let Some(file) = d.file.clone() else { continue };
            let path = if Path::new(&file).is_absolute() {
                PathBuf::from(&file)
            } else {
                module_dir.join(&file)
            };
            if let Ok(relative) = path.strip_prefix(&self.root) {
                d.file = Some(relative.display().to_string());
            }
            if d.source.is_none()
                && let (Some(n), Ok(text)) = (d.line, std::fs::read_to_string(&path))
            {
                d.source = text
                    .lines()
                    .nth(n.saturating_sub(1) as usize)
                    .map(str::to_owned);
            }
        }
    }

    /// Hash of every file that can change the project model.
    fn build_files_hash(&self) -> u64 {
        let mut files = Vec::new();
        collect_build_files(&self.root, 0, &mut files);
        files.sort();
        // DefaultHasher isn't stable across Rust releases; a changed hash only costs a re-probe.
        let mut h = DefaultHasher::new();
        for f in files {
            f.hash(&mut h);
            std::fs::read(&f).unwrap_or_default().hash(&mut h);
        }
        h.finish()
    }
}

#[derive(Serialize, Deserialize)]
struct CachedModel {
    key: u64,
    model: ProjectModel,
}

fn collect_build_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth < 4 && !SKIPPED_DIRS.contains(&name.as_str()) {
                collect_build_files(&path, depth + 1, out);
            }
        } else if name.starts_with("build.gradle")
            || name.starts_with("settings.gradle")
            || name == "gradle.properties"
            || name.ends_with(".versions.toml")
            || name == "gradle-wrapper.properties"
        {
            out.push(path);
        }
    }
}

fn gradle_missing(gradle: &Path, e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::ToolNotFound {
            name: gradle.display().to_string(),
            hint: "add the Gradle wrapper to the project (`gradle wrapper`) or install Gradle"
                .into(),
        }
    } else {
        e.into()
    }
}

/// `:assembleDebug` for a single-module build, `:app:assembleFreeDebug` otherwise.
pub fn assemble_task(app: &AppModule, variant: &Variant) -> String {
    let mut name = variant.name.clone();
    if let Some(first) = name.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    let module = if app.path == ":" {
        ""
    } else {
        app.path.as_str()
    };
    format!("{module}:assemble{name}")
}

impl ProjectModel {
    /// The module and variant to build: the given ones, or the only application module and its
    /// debug variant.
    pub fn select(
        &self,
        module: Option<&str>,
        variant: Option<&str>,
    ) -> Result<(&AppModule, &Variant)> {
        let app = match module {
            Some(m) => {
                let wanted = if m.starts_with(':') {
                    m.to_owned()
                } else {
                    format!(":{m}")
                };
                self.apps
                    .iter()
                    .find(|a| a.path == wanted)
                    .ok_or_else(|| unknown("module", m, self.apps.iter().map(|a| a.path.clone())))?
            }
            None => match self.apps.as_slice() {
                [] => {
                    return Err(Error::ProbeFailed {
                        message: "the project has no module applying com.android.application"
                            .into(),
                    });
                }
                [only] => only,
                many => {
                    return Err(Error::AmbiguousBuildTarget {
                        what: "module".into(),
                        candidates: many.iter().map(|a| a.path.clone()).collect(),
                    });
                }
            },
        };
        let names = || app.variants.iter().map(|v| v.name.clone());
        let variant = match variant {
            Some(v) => app
                .variants
                .iter()
                .find(|x| x.name == v)
                .ok_or_else(|| unknown("variant", v, names()))?,
            None => {
                let debug: Vec<&Variant> = app
                    .variants
                    .iter()
                    .filter(|v| v.build_type.as_deref() == Some("debug"))
                    .collect();
                match (
                    app.variants.iter().find(|v| v.name == "debug"),
                    debug.as_slice(),
                ) {
                    (Some(debug), _) => debug,
                    (None, [only]) => only,
                    (None, []) => app
                        .variants
                        .first()
                        .ok_or_else(|| unknown("variant", "debug", names()))?,
                    (None, many) => {
                        return Err(Error::AmbiguousBuildTarget {
                            what: "variant".into(),
                            candidates: many.iter().map(|v| v.name.clone()).collect(),
                        });
                    }
                }
            }
        };
        Ok((app, variant))
    }
}

fn unknown(what: &str, name: &str, candidates: impl Iterator<Item = String>) -> Error {
    Error::UnknownBuildTarget {
        what: what.into(),
        name: name.into(),
        candidates: candidates.collect(),
    }
}

/// Gradle's summary line: `36 actionable tasks: 36 up-to-date`.
fn all_up_to_date(output: &str) -> bool {
    output.lines().any(|l| {
        let Some((total, rest)) = l.split_once(" actionable task") else {
            return false;
        };
        rest.split_once(": ")
            .and_then(|(_, r)| r.strip_suffix(" up-to-date"))
            .is_some_and(|n| n == total.trim())
    })
}

/// The APKs AGP wrote for `variant`, from its `output-metadata.json` (the same format, version 3,
/// on AGP 7.4, 8.7 and 9.2).
pub fn find_apks(module_dir: &Path, variant: &str) -> Option<ApkSet> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Metadata {
        application_id: String,
        variant_name: String,
        elements: Vec<Element>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Element {
        output_file: String,
        version_name: Option<String>,
        #[serde(default)]
        filters: Vec<Filter>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Filter {
        filter_type: String,
        value: String,
    }

    let mut stack = vec![module_dir.join("build/outputs/apk")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .is_some_and(|n| n == "output-metadata.json")
            {
                let Some(meta) = std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Metadata>(&b).ok())
                else {
                    continue;
                };
                if meta.variant_name == variant {
                    let outputs = meta
                        .elements
                        .into_iter()
                        .map(|e| ApkOutput {
                            path: dir.join(&e.output_file),
                            abis: e
                                .filters
                                .into_iter()
                                .filter(|f| f.filter_type == "ABI")
                                .map(|f| f.value)
                                .collect(),
                            version_name: e.version_name,
                        })
                        .collect();
                    return Some(ApkSet {
                        application_id: meta.application_id,
                        outputs,
                    });
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> ProjectModel {
        // As probed from a two-module project with free/paid flavors.
        let v = |name: &str, id: &str, bt: &str| Variant {
            name: name.into(),
            application_id: Some(id.into()),
            build_type: Some(bt.into()),
        };
        ProjectModel {
            apps: vec![AppModule {
                path: ":app".into(),
                dir: PathBuf::from("/work/mm/app"),
                variants: vec![
                    v("freeDebug", "com.example.mm.free", "debug"),
                    v("paidDebug", "com.example.mm", "debug"),
                    v("freeRelease", "com.example.mm.free", "release"),
                ],
            }],
        }
    }

    #[test]
    fn tasks_for_single_and_multi_module_builds() {
        let m = model();
        let (app, variant) = m.select(Some("app"), Some("freeDebug")).unwrap();
        assert_eq!(assemble_task(app, variant), ":app:assembleFreeDebug");
        let root = AppModule {
            path: ":".into(),
            ..app.clone()
        };
        let debug = Variant {
            name: "debug".into(),
            ..variant.clone()
        };
        assert_eq!(assemble_task(&root, &debug), ":assembleDebug");
    }

    #[test]
    fn several_debug_variants_must_be_chosen() {
        let m = model();
        let err = m.select(None, None).unwrap_err();
        assert!(
            matches!(err, Error::AmbiguousBuildTarget { ref candidates, .. } if candidates == &["freeDebug", "paidDebug"])
        );
        let err = m.select(Some(":lib"), None).unwrap_err();
        assert!(matches!(err, Error::UnknownBuildTarget { .. }));
    }

    #[test]
    fn abi_splits_pick_the_device_abi_then_universal() {
        let dir = std::env::temp_dir().join(format!("mdh-apks-{}", std::process::id()));
        let out = dir.join("build/outputs/apk/free/debug");
        std::fs::create_dir_all(&out).unwrap();
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/android/gradle/output_metadata_abi_splits.json"
        );
        std::fs::copy(fixture, out.join("output-metadata.json")).unwrap();

        let set = find_apks(&dir, "freeDebug").expect("metadata found");
        assert_eq!(set.outputs.len(), 3);
        let arm = set.select(&["arm64-v8a".into()]).unwrap();
        assert!(
            arm.path.ends_with("app-free-arm64-v8a-debug.apk"),
            "{:?}",
            arm.path
        );
        let other = set.select(&["riscv64".into()]).unwrap();
        assert!(
            other.path.ends_with("app-free-universal-debug.apk"),
            "{:?}",
            other.path
        );

        let splits_only = ApkSet {
            outputs: set
                .outputs
                .iter()
                .filter(|o| !o.abis.is_empty())
                .cloned()
                .collect(),
            ..set.clone()
        };
        assert!(matches!(
            splits_only.select(&["riscv64".into()]),
            Err(Error::NoApk { .. })
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn up_to_date_summary() {
        assert!(all_up_to_date(
            "BUILD SUCCESSFUL in 1s\n36 actionable tasks: 36 up-to-date\n"
        ));
        assert!(!all_up_to_date(
            "36 actionable tasks: 5 executed, 31 up-to-date\n"
        ));
        assert!(!all_up_to_date("1 actionable task: 1 executed\n"));
    }
}
