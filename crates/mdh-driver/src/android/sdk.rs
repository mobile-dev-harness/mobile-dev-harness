use std::env;
use std::env::consts::EXE_SUFFIX;
use std::path::PathBuf;

use mdh_core::{Error, Result};

use crate::process::run;

/// Locations of the Android SDK tools the harness needs.
///
/// Users rarely have `emulator` on PATH, so we resolve tools from the SDK root first
/// and fall back to PATH only when no SDK root is found.
#[derive(Debug, Clone)]
pub struct AndroidSdk {
    pub root: Option<PathBuf>,
    pub adb: PathBuf,
    pub emulator: Option<PathBuf>,
}

impl AndroidSdk {
    pub fn locate() -> Result<Self> {
        let root = sdk_root_candidates().into_iter().find(|p| p.is_dir());
        let adb = find_tool(root.as_ref(), "platform-tools", "adb").ok_or_else(|| {
            Error::ToolNotFound {
                name: "adb".into(),
                hint: "install Android SDK platform-tools or set ANDROID_HOME".into(),
            }
        })?;
        let emulator = find_tool(root.as_ref(), "emulator", "emulator");
        Ok(Self {
            root,
            adb,
            emulator,
        })
    }

    /// Names of the Android Virtual Devices available to `emulator`.
    pub async fn avds(&self) -> Result<Vec<String>> {
        let Some(emulator) = &self.emulator else {
            return Ok(Vec::new());
        };
        let out = run(emulator, &["-list-avds"]).await?;
        Ok(parse_avds(&out))
    }
}

fn sdk_root_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        candidates.push(home.join("Library/Android/sdk")); // macOS (Android Studio default)
        candidates.push(home.join("Android/Sdk")); // Linux
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("Android/Sdk")); // Windows
    }
    candidates
}

fn find_tool(root: Option<&PathBuf>, subdir: &str, name: &str) -> Option<PathBuf> {
    let file = format!("{name}{EXE_SUFFIX}");
    root.map(|r| r.join(subdir).join(&file))
        .filter(|p| p.is_file())
        .or_else(|| which::which(name).ok())
}

/// `emulator -list-avds` may interleave log lines (e.g. `INFO | ...`) with AVD names,
/// so keep only lines that are valid AVD names.
fn parse_avds(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_avds_skips_log_lines() {
        let out = "INFO    | Storing crashdata in: /tmp/foo\nPixel_9_Pro_XL\nspec36\n\n";
        assert_eq!(parse_avds(out), ["Pixel_9_Pro_XL", "spec36"]);
    }
}
