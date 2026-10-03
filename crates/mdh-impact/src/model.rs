//! What extraction produces for one file: declarations, references and Android facts.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclKind {
    Class,
    Interface,
    Object,
    Enum,
    Function,
    Constructor,
    Property,
    TypeAlias,
    /// An Android resource: a layout or other resource file, a values entry, a view id.
    Resource,
    /// An entry of `AndroidManifest.xml`: a component, a permission, the application element.
    Manifest,
}

impl DeclKind {
    pub fn is_type(self) -> bool {
        matches!(
            self,
            DeclKind::Class
                | DeclKind::Interface
                | DeclKind::Object
                | DeclKind::Enum
                | DeclKind::TypeAlias
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decl {
    pub kind: DeclKind,
    /// Simple name: `pay`, `LoginActivity`, `activity_login`, `sign_in_label`.
    pub name: String,
    /// Enclosing types, outermost first, joined with `.`; empty at top level.
    pub owner: String,
    /// Identifies the declaration within its file across versions (`Checkout.pay`, overloads get
    /// their parameter types appended).
    pub key: String,
    /// Normalized tokens of what users depend on: modifiers, parameters, return type, supertypes.
    pub signature: String,
    /// Hash of the normalized body; comments and formatting don't change it.
    pub body: u64,
    /// 1-based.
    pub line: usize,
    pub annotations: Vec<String>,
    /// Direct supertypes by simple name.
    pub supertypes: Vec<String>,
    /// Parameter list as written (whitespace collapsed), for showing signature changes.
    pub params: Option<String>,
    /// Arguments a call needs: required parameters, and the maximum (`None` with varargs).
    pub arity: Option<(usize, Option<usize>)>,
    /// Receiver type of an extension function.
    pub extends: Option<String>,
    /// `override`: called by the framework or through a supertype, not by name.
    pub overrides: bool,
    /// Resource type for resources (`layout`, `string`, `id`, …); element for manifest entries.
    pub rtype: Option<String>,
    /// Short value of string resources, shown when it changes.
    pub value: Option<String>,
    /// API levels the declaration branches on (`SDK_INT >= 33`) or requires (`@RequiresApi(33)`),
    /// each as the first level on the newer side of the boundary.
    pub api_levels: Vec<u32>,
}

impl Decl {
    pub fn new(
        kind: DeclKind,
        name: impl Into<String>,
        owner: impl Into<String>,
        line: usize,
    ) -> Self {
        let name = name.into();
        let owner = owner.into();
        let key = if owner.is_empty() {
            name.clone()
        } else {
            format!("{owner}.{name}")
        };
        Decl {
            kind,
            name,
            owner,
            key,
            signature: String::new(),
            body: 0,
            line,
            annotations: Vec::new(),
            supertypes: Vec::new(),
            params: None,
            arity: None,
            extends: None,
            overrides: false,
            rtype: None,
            value: None,
            api_levels: Vec::new(),
        }
    }

    /// `Checkout.pay`, `LoginActivity`, `@string/sign_in_label`.
    pub fn display(&self) -> String {
        match (self.kind, &self.rtype) {
            (DeclKind::Resource, Some(t)) => format!("@{t}/{}", self.name),
            (DeclKind::Manifest, Some(t)) => format!("<{t}> {}", self.name),
            _ => self.key.split('(').next().unwrap_or(&self.key).to_owned(),
        }
    }

    pub fn is_composable(&self) -> bool {
        self.annotations.iter().any(|a| a == "Composable")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefKind {
    Call,
    /// A type used in a declaration, a cast, a supertype or a type argument.
    Type,
    /// `X::class`, `X.class`: in Android code nearly always a navigation target.
    ClassLiteral,
    /// `R.layout.main`, `@string/title`; carries the resource type.
    Resource(String),
    /// Any other use by name: property reads and writes, references to objects and constants.
    Name,
    /// A file name inside a string literal (`"file:///android_asset/page.html"` → `page.html`).
    Literal,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Receiver {
    /// No receiver: `this`, an enclosing scope or a top-level declaration.
    Implicit,
    /// The receiver's type is known from syntax: `Checkout().pay()`, `Log.e()`, a typed property.
    Type(String),
    /// A receiver whose type syntax can't tell.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub kind: RefKind,
    pub name: String,
    pub receiver: Receiver,
    /// 1-based.
    pub line: usize,
    /// Index into [`FileIndex::decls`] of the innermost enclosing declaration.
    pub from: Option<usize>,
    /// Number of arguments of a call.
    pub args: Option<usize>,
    /// For class literals: the view id that triggers the navigation (`open_login`).
    pub trigger: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Kotlin,
    Java,
    /// `res/layout*/`, also `res/menu*/`.
    Layout,
    /// `res/values*/`.
    Values,
    /// `res/navigation*/`.
    Navigation,
    Manifest,
    /// Any other resource file: drawables, mipmaps, fonts, raw, xml.
    Resource,
    /// `src/*/assets/`.
    Asset,
    /// Gradle scripts, version catalogs, properties, R8 rules.
    Build,
    Other,
}

impl FileKind {
    pub fn is_source(self) -> bool {
        matches!(self, FileKind::Kotlin | FileKind::Java)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestComponent {
    /// Simple class name (`LoginActivity`).
    pub class: String,
    /// `activity`, `service`, `receiver`, `provider`.
    pub element: String,
    pub launcher: bool,
    /// `mdhsample://login`.
    pub deep_links: Vec<String>,
}

/// `<action app:destination>` in a navigation graph, between destination classes (simple names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavEdge {
    pub from: String,
    pub to: String,
    /// The action's id.
    pub trigger: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileIndex {
    /// Relative to the project root, `/`-separated.
    pub path: String,
    pub package: String,
    /// Full names; star imports end with `.*`.
    pub imports: Vec<String>,
    pub decls: Vec<Decl>,
    pub refs: Vec<Ref>,
    /// Layouts: view id → label (text as written, or `@string/name`).
    pub labels: Vec<(String, String)>,
    pub components: Vec<ManifestComponent>,
    /// Navigation-graph actions between destination classes.
    pub edges: Vec<NavEdge>,
    /// Syntax errors left after parsing; results in this file may be incomplete.
    pub errors: usize,
}
