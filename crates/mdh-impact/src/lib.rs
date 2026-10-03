//! Change impact analysis (functional design F14, ADR-0010): what a source change reaches and what
//! needs verifying, from syntax alone — no device, no build.
//!
//! The working tree is compared with a base revision; every Kotlin, Java and Android XML file of the
//! project is parsed with tree-sitter into declarations and references; changed declarations are
//! followed through their users up to the screens that show them.

mod git;
mod impact;
mod index;
mod java;
mod kotlin;
mod model;
mod render;
mod report;
mod sdk;
mod source;
mod syntax;
mod xml;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use mdh_core::{Error, Result};

pub use git::Status as FileStatus;
pub use index::Confidence;
pub use model::DeclKind;
pub use render::render;
pub use report::{
    Callers, Change, ChangeKind, CompatFacts, DeclFacts, EdgeChange, ImpactReport, OtherFile,
    ScreenImpact, ScreenKind, SdkLevels, Site, Stats, Verify,
};

use crate::model::{Decl, FileIndex, FileKind};
use crate::source::{Classified, classify};

/// Files larger than this are generated or vendored, not something an agent edits.
const MAX_FILE_BYTES: usize = 2 << 20;

#[derive(Debug, Clone)]
pub struct Options {
    /// A directory inside the project; the analysis covers the Gradle build containing it.
    pub project: PathBuf,
    /// Revision to compare the working tree with.
    pub base: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            project: PathBuf::from("."),
            base: "HEAD".into(),
        }
    }
}

pub fn analyze(options: &Options) -> Result<ImpactReport> {
    let root = project_root(&options.project)?;
    let repo = git::Repo::open(&root)?;
    let commit = repo.resolve(&options.base)?;
    let changes = repo.changes(&options.base)?;

    let started = Instant::now();
    let files = repo.files()?;
    let project = index_project(&root, &files);
    let index_ms = millis(started);

    let analysis = Instant::now();
    let old_paths: Vec<&str> = changes
        .iter()
        .filter(|c| c.status != git::Status::Added)
        .map(|c| c.old_path.as_deref().unwrap_or(&c.path))
        .filter(|p| analyzed(&classify(p)))
        .collect();
    let old = repo.read_at(&options.base, &old_paths)?;
    let mut report = impact::analyze(&project, &changes, &old);
    report.compat.sdk = sdk_levels(&root, &repo, &options.base, &files)?;
    report.compat.qualified_dirs = qualified_dirs(&files);
    for (fi, f) in project.files.iter().enumerate() {
        if project.classes[fi].test {
            continue;
        }
        report
            .compat
            .uses
            .extend(f.refs.iter().map(|r| r.name.clone()));
        report.compat.uses.extend(
            f.decls
                .iter()
                .filter(|d| d.kind == DeclKind::Manifest)
                .map(|d| d.name.clone()),
        );
    }
    report.compat.features = project
        .files
        .iter()
        .flat_map(|f| &f.decls)
        .filter(|d| d.kind == DeclKind::Manifest && d.rtype.as_deref() == Some("uses-feature"))
        .map(|d| d.name.clone())
        .collect();
    report.base = options.base.clone();
    report.base_commit = commit;
    report.stats.files_indexed = project.files.len();
    report.stats.declarations = project.decl_count();
    report.stats.references = project.ref_count();
    report.stats.index_ms = index_ms;
    report.stats.analysis_ms = millis(analysis);
    Ok(report)
}

/// SDK levels of the app module in the base and now, from the build scripts and the catalog.
fn sdk_levels(
    root: &Path,
    repo: &git::Repo,
    base: &str,
    files: &[String],
) -> Result<report::SdkLevels> {
    let paths: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|p| p.ends_with(".gradle.kts") || p.ends_with(".gradle") || *p == sdk::CATALOG)
        .filter(|p| !p.starts_with("settings."))
        .collect();
    let now: std::collections::HashMap<String, Vec<u8>> = paths
        .iter()
        .filter_map(|p| {
            std::fs::read(root.join(p))
                .ok()
                .map(|b| ((*p).to_owned(), b))
        })
        .collect();
    let before = repo.read_at(base, &paths)?;
    Ok(sdk::levels(&before, &now))
}

/// `layout-sw600dp`, `values-night`: qualified resource directories, each once.
fn qualified_dirs(files: &[String]) -> Vec<String> {
    let mut dirs: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let c = classify(f);
            let dir = c.res_dir?;
            (!c.qualifiers.is_empty() && !c.test).then(|| format!("{dir}-{}", c.qualifiers))
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// The Gradle root containing `dir` and the app files (sources, resources, manifests, build
/// scripts) that differ from `HEAD`, uncommitted or untracked, relative to that root.
pub fn changed_app_files(dir: &Path) -> Result<(PathBuf, Vec<String>)> {
    let root = project_root(dir)?;
    let repo = git::Repo::open(&root)?;
    let files = repo
        .changes("HEAD")?
        .into_iter()
        .filter(|c| c.status != git::Status::Deleted)
        .map(|c| c.path)
        .filter(|p| classify(p).kind != FileKind::Other)
        .collect();
    Ok((root, files))
}

/// The directory of the Gradle build containing `dir` (with `settings.gradle(.kts)`), else `dir`.
fn project_root(dir: &Path) -> Result<PathBuf> {
    let dir = dir.canonicalize().map_err(|_| Error::ProjectNotFound {
        dir: dir.display().to_string(),
    })?;
    let gradle_root = dir
        .ancestors()
        .find(|d| d.join("settings.gradle.kts").is_file() || d.join("settings.gradle").is_file());
    Ok(gradle_root.unwrap_or(&dir).to_owned())
}

fn analyzed(c: &Classified) -> bool {
    !matches!(c.kind, FileKind::Build | FileKind::Other)
}

/// Parses every analyzed file, on all cores.
fn index_project(root: &Path, files: &[String]) -> index::Project {
    let work: Vec<(&String, Classified)> = files
        .iter()
        .map(|f| (f, classify(f)))
        .filter(|(_, c)| analyzed(c))
        .collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, FileIndex)>> = Mutex::new(Vec::with_capacity(work.len()));
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut local = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((path, class)) = work.get(i) else {
                        break;
                    };
                    let Ok(bytes) = std::fs::read(root.join(path)) else {
                        continue;
                    };
                    if bytes.len() > MAX_FILE_BYTES {
                        continue;
                    }
                    local.push((i, extract(path, &bytes, class)));
                }
                results
                    .lock()
                    .expect("no panics while holding the lock")
                    .extend(local);
            });
        }
    });
    let mut results = results
        .into_inner()
        .expect("no panics while holding the lock");
    results.sort_by_key(|(i, _)| *i);
    let mut indexed = Vec::with_capacity(results.len());
    let mut classes = Vec::with_capacity(results.len());
    for (i, f) in results {
        classes.push(work[i].1.clone());
        indexed.push(f);
    }
    index::Project::new(indexed, classes)
}

/// Declarations and references of one file.
pub(crate) fn extract(path: &str, bytes: &[u8], class: &Classified) -> FileIndex {
    let text = || String::from_utf8_lossy(bytes);
    match class.kind {
        FileKind::Kotlin => kotlin::extract(path, &text()),
        FileKind::Java => java::extract(path, &text()),
        FileKind::Layout | FileKind::Values | FileKind::Navigation | FileKind::Manifest => {
            xml::extract(path, &text(), class)
        }
        FileKind::Resource if path.ends_with(".xml") => xml::extract(path, &text(), class),
        FileKind::Resource => xml::binary(path, class, content_hash(bytes)),
        FileKind::Asset => {
            let name = path.rsplit('/').next().unwrap_or(path);
            let mut d = Decl::new(DeclKind::Resource, name, "", 1);
            d.rtype = Some("asset".into());
            d.key = format!("@asset/{name}");
            d.body = content_hash(bytes);
            FileIndex {
                path: path.to_owned(),
                decls: vec![d],
                ..FileIndex::default()
            }
        }
        FileKind::Build | FileKind::Other => FileIndex {
            path: path.to_owned(),
            ..FileIndex::default()
        },
    }
}

fn content_hash(bytes: &[u8]) -> u64 {
    syntax::hash_bytes(bytes)
}

fn millis(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}
