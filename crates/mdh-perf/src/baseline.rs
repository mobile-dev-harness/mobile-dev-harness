//! Performance baselines: per device profile and scope (a flow, or `startup-<package>`), the
//! summaries of each metric. Kept in `.mdh/baselines/perf/` with the project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mdh_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::metrics::Metric;
use crate::stats::Summary;

pub const DIR: &str = ".mdh/baselines/perf";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub metrics: BTreeMap<Metric, Summary>,
}

pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    pub fn path(&self, profile: &str, scope: &str) -> PathBuf {
        self.dir.join(profile).join(format!("{scope}.json"))
    }

    pub fn candidate(&self, profile: &str, scope: &str) -> PathBuf {
        self.dir.join(profile).join(format!("{scope}.new.json"))
    }

    pub fn load(path: &Path) -> Option<Baseline> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    pub fn write(path: &Path, baseline: &Baseline) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(baseline).expect("serializable"),
        )?;
        Ok(())
    }

    /// Promotes candidates (all, or one scope's) to baselines.
    pub fn approve(&self, scope: Option<&str>) -> Result<Vec<PathBuf>> {
        let mut approved = Vec::new();
        let Ok(profiles) = std::fs::read_dir(&self.dir) else {
            return Err(Error::InvalidFlow {
                flow: scope.unwrap_or("").to_owned(),
                reason: format!("no performance baselines under {}", self.dir.display()),
            });
        };
        for profile in profiles.flatten() {
            for entry in std::fs::read_dir(profile.path())?.flatten() {
                let p = entry.path();
                let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(stem) = name.strip_suffix(".new.json") else {
                    continue;
                };
                if scope.is_some_and(|s| s != stem) {
                    continue;
                }
                let target = p.with_file_name(format!("{stem}.json"));
                std::fs::rename(&p, &target)?;
                approved.push(target);
            }
        }
        approved.sort();
        Ok(approved)
    }
}
