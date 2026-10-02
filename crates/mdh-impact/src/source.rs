//! Which files the analysis reads, what kind each is, and naming conventions shared by extractors.

use crate::model::{Decl, DeclKind, FileKind};

/// Resource types that can follow `R.` in code or `@` in XML.
pub const RESOURCE_TYPES: &[&str] = &[
    "anim",
    "animator",
    "array",
    "attr",
    "bool",
    "color",
    "dimen",
    "drawable",
    "font",
    "fraction",
    "id",
    "integer",
    "interpolator",
    "layout",
    "menu",
    "mipmap",
    "navigation",
    "plurals",
    "raw",
    "string",
    "style",
    "styleable",
    "transition",
    "xml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub kind: FileKind,
    /// Resource directory type (`layout`, `values`, `drawable`, …).
    pub res_dir: Option<String>,
    /// Resource qualifiers (`zh-rCN`, `land`, `night`); empty for the default configuration.
    pub qualifiers: String,
    /// Under a test source set (`src/test`, `src/androidTest`, …).
    pub test: bool,
}

pub fn classify(path: &str) -> Classified {
    let parts: Vec<&str> = path.split('/').collect();
    let file = parts.last().copied().unwrap_or_default();
    let test = parts
        .windows(2)
        .any(|w| w[0] == "src" && (w[1].starts_with("test") || w[1].starts_with("androidTest")));
    let mut c = Classified {
        kind: FileKind::Other,
        res_dir: None,
        qualifiers: String::new(),
        test,
    };
    let n = parts.len();
    if n >= 3 && parts[n - 3] == "res" && file.contains('.') {
        let (dir, qualifiers) = parts[n - 2].split_once('-').unwrap_or((parts[n - 2], ""));
        c.kind = match dir {
            "layout" | "menu" => FileKind::Layout,
            "values" => FileKind::Values,
            "navigation" => FileKind::Navigation,
            _ => FileKind::Resource,
        };
        c.res_dir = Some(dir.to_owned());
        c.qualifiers = qualifiers.to_owned();
        return c;
    }
    c.kind = if file == "AndroidManifest.xml" {
        FileKind::Manifest
    } else if file.ends_with(".gradle")
        || file.ends_with(".gradle.kts")
        || file.ends_with(".versions.toml")
        || file == "gradle.properties"
        || file.ends_with(".pro")
    {
        FileKind::Build
    } else if file.ends_with(".kt") {
        FileKind::Kotlin
    } else if file.ends_with(".java") {
        FileKind::Java
    } else if parts.windows(3).any(|w| w[0] == "src" && w[2] == "assets") {
        FileKind::Asset
    } else {
        FileKind::Other
    };
    c
}

/// The layout a view-binding class is generated from: `ActivityLoginBinding` → `activity_login`.
pub fn binding_layout(name: &str) -> Option<String> {
    let stem = name.strip_suffix("Binding")?;
    (stem.len() > 1 && stem.starts_with(char::is_uppercase)).then(|| snake_case(stem))
}

/// `signIn` → `sign_in`, `ActivityLogin` → `activity_login`.
pub fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// A file name inside a string literal, the way code refers to assets and raw files:
/// `"file:///android_asset/page.html"` → `page.html`.
pub fn literal_file_name(s: &str) -> Option<String> {
    if s.len() > 200 || s.contains(char::is_whitespace) {
        return None;
    }
    let name = s.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    let ok = !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
        && (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_lowercase());
    ok.then(|| name.to_owned())
}

/// Overloads share a name; their keys get the parameter list so each matches its own old version.
pub fn disambiguate_overloads(decls: &mut [Decl]) {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for d in decls.iter() {
        *counts.entry(d.key.clone()).or_default() += 1;
    }
    for d in decls.iter_mut() {
        if counts[&d.key] > 1 && matches!(d.kind, DeclKind::Function | DeclKind::Constructor) {
            let params = d.params.as_deref().unwrap_or("()");
            d.key = format!("{}{}", d.key, params);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_android_files() {
        let k = |p: &str| classify(p).kind;
        assert_eq!(k("app/src/main/kotlin/a/B.kt"), FileKind::Kotlin);
        assert_eq!(k("app/src/main/res/layout/main.xml"), FileKind::Layout);
        assert_eq!(k("app/src/main/res/menu/main.xml"), FileKind::Layout);
        assert_eq!(
            k("app/src/main/res/drawable-hdpi/icon.png"),
            FileKind::Resource
        );
        assert_eq!(k("app/src/main/AndroidManifest.xml"), FileKind::Manifest);
        assert_eq!(k("app/build.gradle.kts"), FileKind::Build);
        assert_eq!(k("gradle/libs.versions.toml"), FileKind::Build);
        assert_eq!(k("app/src/main/assets/page.html"), FileKind::Asset);
        let v = classify("app/src/main/res/values-zh-rCN/strings.xml");
        assert_eq!(
            (v.kind, v.qualifiers.as_str()),
            (FileKind::Values, "zh-rCN")
        );
        assert!(classify("app/src/androidTest/java/a/BTest.kt").test);
        assert!(!classify("app/src/main/java/a/B.kt").test);
    }

    #[test]
    fn names() {
        assert_eq!(
            binding_layout("ActivityLoginBinding").as_deref(),
            Some("activity_login")
        );
        assert_eq!(binding_layout("Binding"), None);
        assert_eq!(snake_case("signIn"), "sign_in");
        assert_eq!(
            literal_file_name("file:///android_asset/page.html").as_deref(),
            Some("page.html")
        );
        assert_eq!(literal_file_name("Hello world.txt"), None);
        assert_eq!(literal_file_name("1.5"), None);
        assert_eq!(literal_file_name("alice@example.com"), None);
    }
}
