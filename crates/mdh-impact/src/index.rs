//! The whole project's declarations and references, and name resolution over them.

use std::collections::HashMap;

use serde::Serialize;

use crate::model::{Decl, DeclKind, FileIndex, Receiver, Ref, RefKind};
use crate::source::Classified;

/// A declaration: (file, index in that file's `decls`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeclId(pub usize, pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Several candidates share the name; this is one of them.
    Ambiguous,
    /// The only candidate by name.
    Likely,
    /// The only candidate, confirmed by the receiver's type, an import or the enclosing scope.
    Exact,
}

/// More candidates than this for a name with an unknown receiver means the name is too common to
/// say anything (`load`, `update`).
const MAX_UNKNOWN_CANDIDATES: usize = 4;

pub struct Project {
    pub files: Vec<FileIndex>,
    pub classes: Vec<Classified>,
    by_name: HashMap<String, Vec<DeclId>>,
    by_path: HashMap<String, usize>,
    /// Supertype simple name → classes that extend or implement it.
    subtypes: HashMap<String, Vec<DeclId>>,
    /// Resolved uses of each declaration: (file, ref index, confidence).
    users: HashMap<DeclId, Vec<(usize, usize, Confidence)>>,
}

impl Project {
    pub fn new(files: Vec<FileIndex>, classes: Vec<Classified>) -> Self {
        let mut p = Project {
            files,
            classes,
            by_name: HashMap::new(),
            by_path: HashMap::new(),
            subtypes: HashMap::new(),
            users: HashMap::new(),
        };
        for (fi, f) in p.files.iter().enumerate() {
            p.by_path.insert(f.path.clone(), fi);
            for (di, d) in f.decls.iter().enumerate() {
                p.by_name
                    .entry(d.name.clone())
                    .or_default()
                    .push(DeclId(fi, di));
                if d.kind.is_type() {
                    for s in &d.supertypes {
                        p.subtypes
                            .entry(s.clone())
                            .or_default()
                            .push(DeclId(fi, di));
                    }
                }
            }
        }
        let mut users: HashMap<DeclId, Vec<(usize, usize, Confidence)>> = HashMap::new();
        for fi in 0..p.files.len() {
            for ri in 0..p.files[fi].refs.len() {
                let r = &p.files[fi].refs[ri];
                if r.from.is_none() || !p.by_name.contains_key(&r.name) {
                    continue;
                }
                let (targets, confidence) = p.resolve(fi, r);
                for t in targets {
                    // A declaration's own name inside it (recursion, a class naming itself) isn't a use.
                    if t == DeclId(fi, r.from.unwrap_or(usize::MAX)) {
                        continue;
                    }
                    users.entry(t).or_default().push((fi, ri, confidence));
                }
            }
        }
        p.users = users;
        p
    }

    pub fn decl(&self, id: DeclId) -> &Decl {
        &self.files[id.0].decls[id.1]
    }

    pub fn file_of(&self, path: &str) -> Option<usize> {
        self.by_path.get(path).copied()
    }

    pub fn named(&self, name: &str) -> &[DeclId] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    pub fn users(&self, id: DeclId) -> &[(usize, usize, Confidence)] {
        self.users.get(&id).map_or(&[], Vec::as_slice)
    }

    pub fn subtypes(&self, name: &str) -> &[DeclId] {
        self.subtypes.get(name).map_or(&[], Vec::as_slice)
    }

    pub fn decl_count(&self) -> usize {
        self.files.iter().map(|f| f.decls.len()).sum()
    }

    pub fn ref_count(&self) -> usize {
        self.files.iter().map(|f| f.refs.len()).sum()
    }

    /// Types with this simple name.
    pub fn types_named(&self, name: &str) -> impl Iterator<Item = DeclId> + '_ {
        self.named(name)
            .iter()
            .copied()
            .filter(|&id| self.decl(id).kind.is_type())
    }

    /// The top-level type a declaration belongs to (itself, for a top-level type).
    pub fn top_type(&self, id: DeclId) -> Option<DeclId> {
        let d = self.decl(id);
        let top = d.owner.split('.').next().filter(|s| !s.is_empty());
        match top {
            None if d.kind.is_type() => Some(id),
            None => None,
            Some(top) => self.files[id.0]
                .decls
                .iter()
                .position(|c| c.kind.is_type() && c.owner.is_empty() && c.name == top)
                .map(|i| DeclId(id.0, i)),
        }
    }

    /// The type a member is declared in.
    pub fn enclosing_type(&self, id: DeclId) -> Option<DeclId> {
        let d = self.decl(id);
        if d.owner.is_empty() {
            return None;
        }
        let (outer, name) = match d.owner.rsplit_once('.') {
            Some((outer, name)) => (outer, name),
            None => ("", d.owner.as_str()),
        };
        self.files[id.0]
            .decls
            .iter()
            .position(|c| c.kind.is_type() && c.owner == outer && c.name == name)
            .map(|i| DeclId(id.0, i))
    }

    /// Supertypes of a type, transitively, as far as the project declares them.
    pub fn supertypes(&self, id: DeclId) -> Vec<String> {
        let mut out = Vec::new();
        let mut todo = self.decl(id).supertypes.clone();
        while let Some(s) = todo.pop() {
            if out.contains(&s) || out.len() > 16 {
                continue;
            }
            for t in self.types_named(&s) {
                todo.extend(self.decl(t).supertypes.iter().cloned());
            }
            out.push(s);
        }
        out
    }

    /// Which declarations `r` (in file `fi`) refers to, and how sure that is.
    pub fn resolve(&self, fi: usize, r: &Ref) -> (Vec<DeclId>, Confidence) {
        let (targets, confidence) = self.candidates(fi, r);
        if targets.len() < 2 || !matches!(r.kind, RefKind::Call | RefKind::Name) {
            return (targets, confidence);
        }
        // Overloads are one name in one place, not an ambiguity: the argument count usually picks
        // one, and otherwise any of them is the right answer to "who uses this".
        let first = self.decl(targets[0]);
        let overloads = targets.iter().all(|&t| {
            let d = self.decl(t);
            d.kind == DeclKind::Function
                && d.name == first.name
                && d.owner == first.owner
                && self.files[t.0].package == self.files[targets[0].0].package
        });
        if !overloads {
            return (targets, confidence);
        }
        let fits = |t: &DeclId| match (self.decl(*t).arity, r.args) {
            (Some((min, max)), Some(n)) => n >= min && max.is_none_or(|m| n <= m),
            _ => true,
        };
        let fitting: Vec<DeclId> = targets.iter().copied().filter(fits).collect();
        match fitting.len() {
            1 => (fitting, Confidence::Exact),
            0 => (targets, Confidence::Likely),
            _ => (fitting, Confidence::Likely),
        }
    }

    fn candidates(&self, fi: usize, r: &Ref) -> (Vec<DeclId>, Confidence) {
        let all = self.named(&r.name);
        if all.is_empty() {
            return (Vec::new(), Confidence::Exact);
        }
        let file = &self.files[fi];
        let kind_ok = |d: &Decl| match &r.kind {
            RefKind::Resource(t) => d.kind == DeclKind::Resource && d.rtype.as_deref() == Some(t),
            RefKind::Literal => {
                d.kind == DeclKind::Resource && matches!(d.rtype.as_deref(), Some("asset"))
            }
            RefKind::Type | RefKind::ClassLiteral => d.kind.is_type(),
            RefKind::Call => {
                matches!(
                    d.kind,
                    DeclKind::Function | DeclKind::Constructor | DeclKind::Property
                ) || d.kind.is_type()
            }
            RefKind::Name => {
                matches!(d.kind, DeclKind::Property | DeclKind::Function) || d.kind.is_type()
            }
        };
        let candidates: Vec<DeclId> = all
            .iter()
            .copied()
            .filter(|&id| kind_ok(self.decl(id)))
            // Constructors are reached through their class.
            .filter(|&id| self.decl(id).kind != DeclKind::Constructor)
            .collect();
        if candidates.is_empty() {
            return (candidates, Confidence::Exact);
        }
        if matches!(r.kind, RefKind::Resource(_) | RefKind::Literal) {
            return (candidates, Confidence::Exact);
        }
        if matches!(r.kind, RefKind::Type | RefKind::ClassLiteral) {
            return self.pick(fi, candidates);
        }
        match &r.receiver {
            Receiver::Type(t) => {
                let supers: Vec<String> = self
                    .types_named(t)
                    .flat_map(|id| self.supertypes(id))
                    .collect();
                let project_type = self.types_named(t).next().is_some();
                let mut exact = Vec::new();
                let mut likely = Vec::new();
                for id in candidates {
                    let d = self.decl(id);
                    let owner_last = d.owner.rsplit('.').next().unwrap_or("");
                    let in_type = owner_last == t
                        || d.owner.ends_with(&format!("{t}.Companion"))
                        || d.owner == *t;
                    if in_type || d.extends.as_deref() == Some(t.as_str()) {
                        exact.push(id);
                    } else if supers.iter().any(|s| s == owner_last)
                        || (d.extends.is_some() && !project_type)
                    {
                        likely.push(id);
                    }
                }
                match (exact.len(), likely.len()) {
                    (1, _) => (exact, Confidence::Exact),
                    (0, 0) => (Vec::new(), Confidence::Exact),
                    (0, 1) => (likely, Confidence::Likely),
                    (0, _) => (likely, Confidence::Ambiguous),
                    _ => (exact, Confidence::Ambiguous),
                }
            }
            Receiver::Unknown => {
                let members: Vec<DeclId> = candidates
                    .into_iter()
                    .filter(|&id| {
                        let d = self.decl(id);
                        !d.owner.is_empty() || d.extends.is_some()
                    })
                    .collect();
                match members.len() {
                    0 => (members, Confidence::Exact),
                    1 => (members, Confidence::Likely),
                    n if n <= MAX_UNKNOWN_CANDIDATES => (members, Confidence::Ambiguous),
                    _ => (Vec::new(), Confidence::Ambiguous),
                }
            }
            Receiver::Implicit => {
                let scope = r.from.map(|i| {
                    let d = &file.decls[i];
                    if d.kind.is_type() {
                        if d.owner.is_empty() {
                            d.name.clone()
                        } else {
                            format!("{}.{}", d.owner, d.name)
                        }
                    } else {
                        d.owner.clone()
                    }
                });
                let scope = scope.unwrap_or_default();
                // Enclosing types, innermost first: `A.B` → [`A.B`, `A`].
                let mut scopes: Vec<&str> = Vec::new();
                let mut s = scope.as_str();
                while !s.is_empty() {
                    scopes.push(s);
                    s = s.rsplit_once('.').map_or("", |(outer, _)| outer);
                }
                let inherited: Vec<String> = scopes
                    .iter()
                    .filter_map(|s| {
                        let (outer, name) = s.rsplit_once('.').unwrap_or(("", s));
                        file.decls
                            .iter()
                            .position(|d| d.kind.is_type() && d.owner == outer && d.name == name)
                            .map(|i| DeclId(fi, i))
                    })
                    .flat_map(|id| self.supertypes(id))
                    .collect();
                let mut in_scope = Vec::new();
                let mut visible = Vec::new();
                for id in candidates {
                    let d = self.decl(id);
                    let same_package = self.files[id.0].package == file.package;
                    if d.owner.is_empty() {
                        if id.0 == fi {
                            in_scope.push(id);
                        } else if same_package || self.imported(fi, id) {
                            visible.push(id);
                        }
                    } else if same_package
                        && scopes
                            .iter()
                            .any(|s| d.owner == *s || d.owner == format!("{s}.Companion"))
                    {
                        in_scope.push(id);
                    } else if inherited
                        .iter()
                        .any(|s| d.owner.rsplit('.').next() == Some(s.as_str()))
                    {
                        visible.push(id);
                    } else if d.kind.is_type() && (same_package || self.imported(fi, id)) {
                        // A nested type named without its outer type (imported or in scope).
                        visible.push(id);
                    }
                }
                if !in_scope.is_empty() {
                    let c = if in_scope.len() == 1 {
                        Confidence::Exact
                    } else {
                        Confidence::Ambiguous
                    };
                    return (in_scope, c);
                }
                self.pick(fi, visible)
            }
        }
    }

    /// Narrows type candidates by file, import and package.
    fn pick(&self, fi: usize, candidates: Vec<DeclId>) -> (Vec<DeclId>, Confidence) {
        let package = &self.files[fi].package;
        if candidates.len() <= 1 {
            let c = if candidates.iter().any(|id| {
                id.0 == fi || self.imported(fi, *id) || &self.files[id.0].package == package
            }) {
                Confidence::Exact
            } else {
                Confidence::Likely
            };
            return (candidates, c);
        }
        for narrowed in [
            candidates
                .iter()
                .copied()
                .filter(|id| id.0 == fi)
                .collect::<Vec<_>>(),
            candidates
                .iter()
                .copied()
                .filter(|id| self.imported(fi, *id))
                .collect(),
            candidates
                .iter()
                .copied()
                .filter(|id| &self.files[id.0].package == package)
                .collect(),
        ] {
            if narrowed.len() == 1 {
                return (narrowed, Confidence::Exact);
            }
        }
        (candidates, Confidence::Ambiguous)
    }

    /// File `fi` imports declaration `id` by name or with a star import of its package.
    fn imported(&self, fi: usize, id: DeclId) -> bool {
        let d = self.decl(id);
        let package = &self.files[id.0].package;
        let container = if d.owner.is_empty() {
            package.clone()
        } else {
            format!("{package}.{}", d.owner)
        };
        let full = format!("{container}.{}", d.name);
        self.files[fi]
            .imports
            .iter()
            .any(|i| *i == full || i.strip_suffix(".*") == Some(container.as_str()))
    }
}
