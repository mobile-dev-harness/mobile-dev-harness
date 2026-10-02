//! Project: understands the app's source project so the other domains can build and run it.
//!
//! Gradle today (functional design F2): find the build, probe its application modules and
//! variants with an injected init script (cached per build-file hash), build incrementally, parse
//! failures into structured diagnostics and locate the APKs. React Native, Expo, Flutter and Xcode
//! adapters come later; a common trait is introduced with the second one.

pub mod diagnostics;
pub mod gradle;
mod render;

pub use diagnostics::{Diagnostic, DiagnosticKind, Severity};
pub use gradle::{
    Apk, ApkOutput, ApkSet, AppModule, BuildOutcome, GradleProject, ProjectModel, Variant,
    assemble_task, find_apks,
};
pub use render::render_build;
