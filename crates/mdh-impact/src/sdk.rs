//! The app module's `minSdk`, `targetSdk` and `compileSdk`, read from the Gradle scripts as text
//! (no Gradle run): literals, and `libs.versions.…` references into the version catalog.

use std::collections::HashMap;

use crate::report::SdkLevels;

pub const CATALOG: &str = "gradle/libs.versions.toml";

/// Levels in the base and now; `base` and `now` map paths of build scripts and the catalog to
/// their contents.
pub fn levels(base: &HashMap<String, Vec<u8>>, now: &HashMap<String, Vec<u8>>) -> SdkLevels {
    let (b, n) = (read(base), read(now));
    SdkLevels {
        min: (b.0, n.0),
        target: (b.1, n.1),
        compile: (b.2, n.2),
    }
}

type Levels = (Option<u32>, Option<u32>, Option<u32>);

fn read(files: &HashMap<String, Vec<u8>>) -> Levels {
    let text = |p: &str| {
        files
            .get(p)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    };
    let catalog = text(CATALOG).map(|c| versions(&c)).unwrap_or_default();
    let mut scripts: Vec<(&String, String)> = files
        .keys()
        .filter(|p| p.ends_with("build.gradle.kts") || p.ends_with("build.gradle"))
        .filter_map(|p| text(p).map(|t| (p, t)))
        .collect();
    // The application module's script first; library modules can set other levels.
    scripts.sort_by_key(|(p, t)| (!is_application(t), p.len()));
    let find = |keys: &[&str]| scripts.iter().find_map(|(_, t)| level(t, keys, &catalog));
    (
        find(&["minSdkVersion", "minSdk"]),
        find(&["targetSdkVersion", "targetSdk"]),
        find(&["compileSdkVersion", "compileSdk"]),
    )
}

fn is_application(script: &str) -> bool {
    script.contains("com.android.application") || script.contains("android.application")
}

/// The first assignment of one of `keys`: `minSdk = 26`, `minSdkVersion 21`,
/// `compileSdk { version = release(36) }`, `targetSdk = libs.versions.targetSdk.get().toInt()`.
fn level(script: &str, keys: &[&str], catalog: &HashMap<String, String>) -> Option<u32> {
    for line in script.lines() {
        let line = line.split("//").next().unwrap_or_default().trim();
        for key in keys {
            let Some(rest) = line.strip_prefix(key) else {
                continue;
            };
            // `minSdkPreview`, `targetSdkVersionCode` and the like aren't the level.
            if rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
                continue;
            }
            if let Some(n) = first_number(rest) {
                return Some(n);
            }
            if let Some(i) = rest.find("libs.versions.") {
                let name: String = rest[i + "libs.versions.".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
                    .collect();
                let name = name.trim_end_matches(".get");
                return catalog.get(&normalize(name)).and_then(|v| first_number(v));
            }
        }
    }
    None
}

fn first_number(s: &str) -> Option<u32> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok().filter(|n| (1..100).contains(n))
}

/// `[versions]` of the catalog, keys normalized (`min-sdk`, `minSdk` and `min.sdk` are one key).
fn versions(catalog: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_versions = false;
    for line in catalog.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_versions = line == "[versions]";
            continue;
        }
        if let (true, Some((k, v))) = (in_versions, line.split_once('=')) {
            out.insert(normalize(k.trim()), v.trim().trim_matches('"').to_owned());
        }
    }
    out
}

fn normalize(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(entries: &[(&str, &str)]) -> HashMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn literals_catalog_references_and_old_syntax() {
        let base = files(&[
            (
                "app/build.gradle",
                "plugins { id 'com.android.application' }\nandroid {\n  compileSdkVersion 34\n  defaultConfig {\n    minSdkVersion 21\n    targetSdkVersion 33 // old\n  }\n}",
            ),
            (
                "lib/build.gradle",
                "plugins { id 'com.android.library' }\nandroid { defaultConfig { minSdkVersion 19 } }",
            ),
        ]);
        let now = files(&[
            (
                "app/build.gradle.kts",
                "plugins { alias(libs.plugins.android.application) }\nandroid {\n  compileSdk { version = release(36) }\n  defaultConfig {\n    minSdk = libs.versions.min.sdk.get().toInt()\n    targetSdk = 35\n    minSdkPreview = \"x\"\n  }\n}",
            ),
            (
                CATALOG,
                "[versions]\nmin-sdk = \"24\"\nagp = \"8.9.0\"\n[libraries]\nminSdk = \"x\"",
            ),
        ]);
        let l = levels(&base, &now);
        assert_eq!(l.min, (Some(21), Some(24)));
        assert_eq!(l.target, (Some(33), Some(35)));
        assert_eq!(l.compile, (Some(34), Some(36)));
    }
}
