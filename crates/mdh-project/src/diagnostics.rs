//! Turns Gradle build output into structured diagnostics (functional design F2.3).
//!
//! Pure functions over the captured output; tested with real failures of the sample app in
//! `fixtures/android/gradle/`. Compiler messages may be localized (javac follows the system
//! locale), so formats are recognized by shape, not by English keywords.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticKind {
    Kotlin,
    Java,
    Resource,
    Manifest,
    Dependency,
    /// Anything else Gradle reported under `* What went wrong:`.
    Gradle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    /// As reported: an absolute path, or a resource path like `res/layout/main.xml` (see `parse`).
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
    /// Extra lines the tool printed: javac's symbol details, manifest merger suggestions, …
    pub notes: Vec<String>,
    /// The offending source line, when known (read from disk later for Kotlin).
    pub source: Option<String>,
}

impl Diagnostic {
    fn new(kind: DiagnosticKind, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            kind,
            file: None,
            line: None,
            column: None,
            message: message.into(),
            notes: Vec::new(),
            source: None,
        }
    }
}

/// All diagnostics in `output`, in order of appearance, duplicates removed (Gradle repeats
/// compiler output under `* What went wrong:`). The generic Gradle failure is only reported when
/// nothing more specific explains it.
pub fn parse(output: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = output.lines().collect();
    let mut out: Vec<Diagnostic> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let (found, consumed) = kotlin(line)
            .map(|d| (vec![d], 1))
            .or_else(|| java(&lines[i..]))
            .or_else(|| aapt(line).map(|d| (vec![d], 1)))
            .or_else(|| manifest(&lines[i..]))
            .unwrap_or_default();
        for d in found {
            if !out.iter().any(|o| same(o, &d)) {
                out.push(d);
            }
        }
        i += consumed.max(1);
    }
    out.extend(dependencies(output));
    if !out.iter().any(|d| d.severity == Severity::Error) {
        out.extend(what_went_wrong(output));
    }
    out
}

fn same(a: &Diagnostic, b: &Diagnostic) -> bool {
    a.kind == b.kind && a.file == b.file && a.line == b.line && a.message == b.message
}

/// `e: file:///work/app/src/main/kotlin/Foo.kt:29:9 Unresolved reference 'emial'.` (`w:` for warnings).
fn kotlin(line: &str) -> Option<Diagnostic> {
    let line = line.trim_start();
    let (severity, rest) = match line.strip_prefix("e: ") {
        Some(r) => (Severity::Error, r),
        None => (Severity::Warning, line.strip_prefix("w: ")?),
    };
    let rest = rest.strip_prefix("file://").unwrap_or(rest);
    let (location, message) = rest.split_once(' ')?;
    let mut parts = location.rsplitn(3, ':');
    let (column, line_no, file) = (parts.next()?, parts.next()?, parts.next()?);
    let mut d = Diagnostic::new(DiagnosticKind::Kotlin, severity, message.trim());
    d.file = Some(file.to_owned());
    d.line = line_no.parse().ok();
    d.column = column.parse().ok();
    Some(d)
}

/// `/work/app/src/main/java/Foo.java:5: error: incompatible types…` followed by the source line, a
/// caret line and indented details. The severity word is localized (`错误`, `Fehler`, …).
fn java(lines: &[&str]) -> Option<(Vec<Diagnostic>, usize)> {
    let first = lines[0].trim_start();
    let at = first.find(".java:")? + ".java".len();
    let (file, rest) = first.split_at(at);
    let rest = rest.strip_prefix(':')?;
    let (line_no, rest) = rest.split_once(':')?;
    let line_no: u32 = line_no.trim().parse().ok()?;
    let (word, message) = rest.trim_start().split_once(':')?;
    let severity = if is_warning_word(word.trim()) {
        Severity::Warning
    } else {
        Severity::Error
    };
    let mut d = Diagnostic::new(DiagnosticKind::Java, severity, message.trim());
    d.file = Some(file.to_owned());
    d.line = Some(line_no);

    // Source line, caret line, then indented detail lines until the next message.
    let mut consumed = 1;
    if let Some(source) = lines.get(1) {
        d.source = Some(source.trim_end().to_owned());
        consumed += 1;
    }
    if let Some(caret) = lines.get(2) {
        if caret.trim() == "^" {
            d.column = u32::try_from(caret.find('^').unwrap_or(0) + 1).ok();
            consumed += 1;
        }
    }
    while let Some(next) = lines.get(consumed) {
        let indented = next.starts_with("  ") && !next.trim_start().contains(".java:");
        if !indented || next.trim().is_empty() {
            break;
        }
        d.notes.push(next.trim().to_owned());
        consumed += 1;
    }
    Some((vec![d], consumed))
}

fn is_warning_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "warning"
            | "警告"
            | "warnung"
            | "avertissement"
            | "advertencia"
            | "aviso"
            | "предупреждение"
    )
}

/// `dev.mdh.sample-main-38:/layout/activity_login.xml:33: error: resource string/x … not found.`
/// The prefix names a resource set (`<package>-<source set>-<n>`); the file is reported as
/// `src/<source set>/res/layout/activity_login.xml`, relative to the module.
fn aapt(line: &str) -> Option<Diagnostic> {
    let line = line.trim_start();
    let (set, rest) = line.split_once(":/")?;
    let source_set = set.rsplit('-').nth(1)?;
    if set.contains(' ') || !rest.contains(": error: ") && !rest.contains(": warn: ") {
        return None;
    }
    let (location, message) = rest
        .split_once(": error: ")
        .map(|(l, m)| (l, (Severity::Error, m)))
        .or_else(|| {
            rest.split_once(": warn: ")
                .map(|(l, m)| (l, (Severity::Warning, m)))
        })?;
    let (path, line_no) = location.rsplit_once(':')?;
    let mut d = Diagnostic::new(DiagnosticKind::Resource, message.0, message.1.trim());
    d.file = Some(format!("src/{source_set}/res/{path}"));
    d.line = line_no.parse().ok();
    Some(d)
}

/// `/work/app/src/main/AndroidManifest.xml Error:` followed by tab-indented message and suggestions.
fn manifest(lines: &[&str]) -> Option<(Vec<Diagnostic>, usize)> {
    let first = lines[0].trim();
    let file = first
        .strip_suffix(" Error:")
        .or_else(|| first.strip_suffix(" Warning:"))?;
    if !file.ends_with("AndroidManifest.xml") {
        return None;
    }
    let severity = if first.ends_with("Error:") {
        Severity::Error
    } else {
        Severity::Warning
    };
    let detail: Vec<&str> = lines[1..]
        .iter()
        .take_while(|l| l.starts_with('\t'))
        .map(|l| l.trim())
        .collect();
    let mut d = Diagnostic::new(
        DiagnosticKind::Manifest,
        severity,
        detail.first().copied().unwrap_or("manifest merger failed"),
    );
    d.file = Some(file.to_owned());
    d.notes = detail.iter().skip(1).map(|s| (*s).to_owned()).collect();
    Some((vec![d], 1 + detail.len()))
}

/// `Could not find androidx.core:core-ktx:99.0.0.` repeats once per dependency path; report each
/// missing artifact once, with the first path that requires it.
fn dependencies(output: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = output.lines().collect();
    let mut out: Vec<Diagnostic> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(artifact) = line
            .trim_start_matches([' ', '>'])
            .strip_prefix("Could not find ")
            .map(|a| a.trim_end_matches('.'))
        else {
            continue;
        };
        if out.iter().any(|d| d.message.ends_with(artifact)) {
            continue;
        }
        let mut d = Diagnostic::new(
            DiagnosticKind::Dependency,
            Severity::Error,
            format!("could not find {artifact}"),
        );
        if let Some(by) = lines[i..]
            .iter()
            .position(|l| l.trim() == "Required by:")
            .and_then(|p| lines.get(i + p + 1))
        {
            d.notes.push(format!("required by {}", by.trim()));
        }
        out.push(d);
    }
    out
}

/// The `* What went wrong:` paragraph, minus Gradle's wrapper lines.
fn what_went_wrong(output: &str) -> Option<Diagnostic> {
    let start = output.find("* What went wrong:")? + "* What went wrong:".len();
    let end = output[start..]
        .find("\n* ")
        .map_or(output.len(), |e| start + e);
    let text: Vec<&str> = output[start..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let (first, rest) = text.split_first()?;
    let mut d = Diagnostic::new(DiagnosticKind::Gradle, Severity::Error, *first);
    d.notes = rest
        .iter()
        .map(|l| l.trim_start_matches("> ").to_owned())
        .take(6)
        .collect();
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/../../fixtures/android/gradle/{name}.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap()
    }

    fn errors(name: &str) -> Vec<Diagnostic> {
        parse(&fixture(name))
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    #[test]
    fn kotlin_errors() {
        let d = errors("kotlin_errors");
        assert_eq!(d.len(), 2, "{d:#?}");
        assert_eq!(d[0].kind, DiagnosticKind::Kotlin);
        assert_eq!(
            d[0].file.as_deref(),
            Some("/work/android-sample/src/main/kotlin/dev/mdh/sample/LoginActivity.kt")
        );
        assert_eq!((d[0].line, d[0].column), (Some(29), Some(9)));
        assert_eq!(d[0].message, "Unresolved reference 'emial'.");
        assert!(d[1].message.starts_with("Assignment type mismatch"));
    }

    #[test]
    fn javac_errors_in_any_language() {
        // Captured with a Chinese system locale: javac localizes the severity word.
        let d = errors("javac_errors");
        assert_eq!(
            d.len(),
            2,
            "duplicates under `What went wrong` are dropped: {d:#?}"
        );
        assert_eq!(d[0].kind, DiagnosticKind::Java);
        assert_eq!((d[0].line, d[0].column), (Some(5), Some(20)));
        assert_eq!(d[0].source.as_deref(), Some("        String s = 1;"));
        assert_eq!(d[1].line, Some(6));
        assert_eq!(
            d[1].notes.len(),
            2,
            "symbol and location details: {:?}",
            d[1].notes
        );
    }

    #[test]
    fn missing_resource_maps_to_the_source_set() {
        let d = errors("aapt_missing_resource");
        assert_eq!(d.len(), 1, "{d:#?}");
        assert_eq!(d[0].kind, DiagnosticKind::Resource);
        assert_eq!(
            d[0].file.as_deref(),
            Some("src/main/res/layout/activity_login.xml")
        );
        assert_eq!(d[0].line, Some(33));
        assert!(d[0].message.starts_with("resource string/sign_in_label"));
    }

    #[test]
    fn manifest_merger_keeps_the_suggestion() {
        let d = errors("manifest_merger");
        assert_eq!(d.len(), 1, "{d:#?}");
        assert_eq!(d[0].kind, DiagnosticKind::Manifest);
        assert!(
            d[0].message
                .starts_with("uses-sdk:minSdkVersion 19 cannot be smaller than version 21")
        );
        assert!(d[0].notes[0].starts_with("Suggestion:"));
    }

    #[test]
    fn missing_dependency_is_reported_once() {
        let d = errors("dependency_resolution");
        assert_eq!(d.len(), 1, "{d:#?}");
        assert_eq!(d[0].message, "could not find androidx.core:core-ktx:99.0.0");
        assert_eq!(d[0].notes, ["required by root project 'mdh-sample'"]);
    }

    #[test]
    fn generic_gradle_failure_as_a_fallback() {
        let out = "FAILURE: Build failed with an exception.\n\n* What went wrong:\nA problem occurred evaluating root project 'x'.\n> Plugin with id 'nope' not found.\n\n* Try:\n> Run with --stacktrace\n";
        let d = parse(out);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, DiagnosticKind::Gradle);
        assert_eq!(
            d[0].message,
            "A problem occurred evaluating root project 'x'."
        );
        assert_eq!(d[0].notes, ["Plugin with id 'nope' not found."]);
    }
}
