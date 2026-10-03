//! The result of `mdh impact`: serialized as the envelope's `data`, rendered as text by [`crate::render`].

use serde::Serialize;

use crate::git::Status;
use crate::index::Confidence;
use crate::model::DeclKind;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImpactReport {
    /// The base as given (`HEAD`, `main`) and the commit it named.
    pub base: String,
    pub base_commit: String,
    /// Files that differ from the base, analyzed or not.
    pub files_changed: usize,
    pub changes: Vec<Change>,
    /// Changed files whose content was the same apart from comments and formatting.
    pub cosmetic: Vec<String>,
    /// Changed files that aren't analyzed: build scripts, assets nobody references, other files.
    pub other_files: Vec<OtherFile>,
    /// What changed declarations call, navigate to and reference, before vs. after.
    pub edges: Vec<EdgeChange>,
    pub screens: Vec<ScreenImpact>,
    /// Callers of declarations whose signature changed.
    pub callers: Vec<Callers>,
    /// Places that still use something removed.
    pub dangling: Vec<Callers>,
    pub verify: Verify,
    /// What this analysis can't see, and files it couldn't fully parse.
    pub limits: Vec<String>,
    pub stats: Stats,
    /// What compatibility analysis reads (ADR-0011); `mdh-compat` turns it into risks.
    #[serde(skip)]
    pub compat: CompatFacts,
}

/// Facts about the change and the project for compatibility analysis, from syntax alone.
#[derive(Debug, Clone, Default)]
pub struct CompatFacts {
    /// Every changed (or added) declaration outside tests.
    pub decls: Vec<DeclFacts>,
    pub sdk: SdkLevels,
    /// Resource directories with qualifiers in the project: `layout-sw600dp`, `values-night`.
    pub qualified_dirs: Vec<String>,
    /// `<uses-feature>` names in the manifests: `android.hardware.type.automotive`.
    pub features: Vec<String>,
    /// Every name the app's (non-test) sources use, to tell whether a behavior change brought by
    /// a new `targetSdk` touches the app at all.
    pub uses: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct DeclFacts {
    pub decl: String,
    pub file: String,
    pub line: usize,
    pub kind: DeclKind,
    pub change: ChangeKind,
    /// Resource type, or the manifest element.
    pub rtype: Option<String>,
    /// Resource qualifiers of the file (`sw600dp-land`); empty for the default configuration.
    pub qualifiers: String,
    /// Manifest entries: the attributes before and after (`screenOrientation=portrait …`).
    pub before: Option<String>,
    pub after: Option<String>,
    pub annotations: Vec<String>,
    /// Its own and its enclosing type's supertypes.
    pub supertypes: Vec<String>,
    /// Names it uses (calls, types, constants), and `Receiver.name` where the receiver is known.
    pub uses: Vec<String>,
    /// API levels it branches on or requires.
    pub api_levels: Vec<u32>,
    /// Screens it reaches.
    pub screens: Vec<String>,
}

/// The app module's SDK levels in the base and now; `None` when not found or not a literal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SdkLevels {
    pub min: (Option<u32>, Option<u32>),
    pub target: (Option<u32>, Option<u32>),
    pub compile: (Option<u32>, Option<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    /// What callers depend on changed: parameters, return type, supertypes, modifiers.
    Signature,
    /// Only the implementation (or a resource's content) changed.
    Body,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub file: String,
    pub line: usize,
    /// `Checkout.pay`, `@string/sign_in`, `<activity> LoginActivity`.
    pub decl: String,
    pub kind: DeclKind,
    pub change: ChangeKind,
    /// Parameters or value before and after, when they changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OtherFile {
    pub path: String,
    pub status: Status,
    /// `build`, `asset`, `other`.
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct EdgeChange {
    pub decl: String,
    /// `Checkout.validate()`, `MessagesActivity::class`, `@string/title`.
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenKind {
    Activity,
    Fragment,
    /// A composable named `…Screen` or `…Route`.
    Composable,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenImpact {
    pub screen: String,
    pub kind: ScreenKind,
    pub file: String,
    /// From the change to the screen: `["padForSystemBars", "LoginActivity.onCreate"]`.
    pub via: Vec<String>,
    pub confidence: Confidence,
    /// The activity showing a composable screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// How many of the changes reach this screen; `via` shows the most telling one.
    pub changes: usize,
    /// Ways to get there: deep links, then taps from the launcher screen.
    pub reach: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Callers {
    pub decl: String,
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Site {
    pub file: String,
    pub line: usize,
    /// The declaration containing the use.
    pub from: String,
    pub confidence: Confidence,
    /// `1 argument, needs 2`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Verify {
    /// Screens whose behavior may have changed.
    pub functional: Vec<String>,
    /// Changed layouts, resources, composables and views, with the screens that show them.
    pub ui: Vec<String>,
    pub performance: Vec<String>,
    pub compatibility: Vec<String>,
    /// Test classes that use changed or affected code.
    pub tests: Vec<String>,
    /// Saved flows that pass an affected screen; filled in by the verification engine, which owns
    /// flows.
    pub flows: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub files_indexed: usize,
    pub declarations: usize,
    pub references: usize,
    pub index_ms: u64,
    pub analysis_ms: u64,
}
