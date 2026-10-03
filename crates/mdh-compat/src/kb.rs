//! The knowledge base (ADR-0011): Android behavior changes, form-factor triggers and vendor quirks,
//! each entry with its source. It lives in its own repository (`android-compat-kb`, ADR-0012); a
//! pinned release is compiled into the binary (`kb/android.yaml`, version and checksum in
//! `kb/SOURCE`), and `MDH_COMPAT_KB` points a binary at another copy.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kb {
    pub behavior: Vec<Behavior>,
    pub form_factors: Vec<FormFactor>,
    pub vendors: Vec<VendorQuirk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum By {
    /// On devices running the version, whatever the app targets.
    Device,
    /// Once the app's `targetSdk` reaches the version.
    Target,
}

/// What a changed declaration has to use, have or declare to run into an entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Triggers {
    /// Names used or overridden: `PendingIntent.getActivity` (on a known receiver), `notify`.
    #[serde(default)]
    pub uses: Vec<String>,
    /// Manifest elements (`<service>`) or attributes (`foregroundServiceType`).
    #[serde(default)]
    pub manifest: Vec<String>,
    /// Resource qualifiers (`sw600dp`).
    #[serde(default)]
    pub qualifiers: Vec<String>,
    /// `<uses-feature>` names.
    #[serde(default)]
    pub features: Vec<String>,
}

/// `deny_unknown_fields` doesn't work with `flatten`; the tests check the entries instead.
#[derive(Debug, Clone, Deserialize)]
pub struct Behavior {
    pub id: String,
    pub api: u32,
    pub by: By,
    #[serde(flatten)]
    pub triggers: Triggers,
    /// Verify below `api` too.
    #[serde(default)]
    pub both_sides: bool,
    pub summary: String,
    pub verify: String,
    pub source: String,
}

/// A configuration that makes one device look like another kind.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// The device as it is.
    Default,
    /// A small phone, 360×640 dp.
    Compact,
    /// The device turned to landscape.
    Landscape,
    /// A foldable's inner screen, 673×841 dp.
    Foldable,
    /// A tablet in landscape, 1280×800 dp.
    Tablet,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::Default => "default",
            Shape::Compact => "compact",
            Shape::Landscape => "landscape",
            Shape::Foldable => "foldable",
            Shape::Tablet => "tablet",
        }
    }

    /// Width and height in dp the display is set to, `None` for the device's own.
    pub fn size_dp(self) -> Option<(u32, u32)> {
        match self {
            Shape::Compact => Some((360, 640)),
            Shape::Foldable => Some((673, 841)),
            Shape::Tablet => Some((1280, 800)),
            Shape::Default | Shape::Landscape => None,
        }
    }

    pub fn describe(self) -> String {
        match self.size_dp() {
            Some((w, h)) => format!("{} {w}×{h} dp", self.name()),
            None => self.name().to_owned(),
        }
    }
}

/// `deny_unknown_fields` doesn't work with `flatten`; the tests check the entries instead.
#[derive(Debug, Clone, Deserialize)]
pub struct FormFactor {
    pub id: String,
    #[serde(flatten)]
    pub triggers: Triggers,
    #[serde(default)]
    pub cells: Vec<Shape>,
    /// Check that state survives a rotation.
    #[serde(default)]
    pub state: bool,
    /// What a device would need, when no configuration can stand in for it.
    #[serde(default)]
    pub needs: Option<String>,
    pub summary: String,
    pub verify: String,
    pub source: String,
}

/// `deny_unknown_fields` doesn't work with `flatten`; the tests check the entries instead.
#[derive(Debug, Clone, Deserialize)]
pub struct VendorQuirk {
    pub id: String,
    /// Lowercase manufacturer names as devices report them (`ro.product.manufacturer`).
    pub vendors: Vec<String>,
    #[serde(flatten)]
    pub triggers: Triggers,
    pub summary: String,
    pub verify: String,
    pub source: String,
}

const BUILT_IN: &str = include_str!("../kb/android.yaml");

/// The knowledge base: the file `MDH_COMPAT_KB` names, else the one shipped with this build. A file
/// that can't be read or parsed falls back to the built-in one, with a warning on stderr.
pub fn kb() -> &'static Kb {
    static KB: OnceLock<Kb> = OnceLock::new();
    KB.get_or_init(|| {
        if let Some(path) = std::env::var_os("MDH_COMPAT_KB") {
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| serde_norway::from_str(&text).map_err(|e| e.to_string()));
            match parsed {
                Ok(kb) => return kb,
                Err(e) => eprintln!(
                    "warning: MDH_COMPAT_KB={}: {e}; using the built-in knowledge base",
                    std::path::Path::new(&path).display()
                ),
            }
        }
        serde_norway::from_str(BUILT_IN).expect("kb/android.yaml is valid (checked by tests)")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_is_the_release_it_says_it_is() {
        use sha2::{Digest, Sha256};
        let source = include_str!("../kb/SOURCE");
        let pinned = source
            .lines()
            .find_map(|l| l.strip_prefix("sha256 = "))
            .expect("kb/SOURCE names a checksum");
        let actual = format!("{:x}", Sha256::digest(BUILT_IN.as_bytes()));
        assert_eq!(
            actual, pinned,
            "kb/android.yaml was edited in place: change android-compat-kb, release it, then run \
             scripts/update-kb.sh <version>"
        );
    }

    #[test]
    fn the_knowledge_base_parses_and_every_entry_is_sourced() {
        let kb = kb();
        assert!(kb.behavior.len() >= 20);
        let mut ids = std::collections::HashSet::new();
        for (id, source, triggers) in kb
            .behavior
            .iter()
            .map(|b| (&b.id, &b.source, &b.triggers))
            .chain(
                kb.form_factors
                    .iter()
                    .map(|f| (&f.id, &f.source, &f.triggers)),
            )
            .chain(kb.vendors.iter().map(|v| (&v.id, &v.source, &v.triggers)))
        {
            assert!(ids.insert(id.clone()), "duplicate id {id}");
            assert!(source.starts_with("https://"), "{id}: {source}");
            let t = triggers;
            assert!(
                !(t.uses.is_empty()
                    && t.manifest.is_empty()
                    && t.qualifiers.is_empty()
                    && t.features.is_empty()),
                "{id} has no triggers"
            );
        }
        for f in &kb.form_factors {
            assert!(
                f.cells.is_empty() != f.needs.is_none(),
                "{}: cells or needs",
                f.id
            );
        }
        for v in &kb.vendors {
            assert!(v.vendors.iter().all(|n| n == &n.to_lowercase()), "{}", v.id);
        }
    }
}
