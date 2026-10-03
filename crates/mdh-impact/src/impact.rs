//! From changed files to changed declarations, their users, the screens they reach and what to verify.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::git::{FileChange, Status};
use crate::index::{Confidence, DeclId, Project};
use crate::model::{Decl, DeclKind, FileIndex, FileKind, Receiver, Ref, RefKind};
use crate::report::{
    Callers, Change, ChangeKind, DeclFacts, EdgeChange, ImpactReport, OtherFile, ScreenImpact,
    ScreenKind, Site, Verify,
};
use crate::source::classify;

/// How far users are followed from a change.
const MAX_DEPTH: usize = 12;
/// How far a navigation's source is followed up to the screen it starts on.
const SOURCE_DEPTH: usize = 6;

const LAZY_LISTS: &[&str] = &[
    "LazyColumn",
    "LazyRow",
    "LazyVerticalGrid",
    "LazyHorizontalGrid",
    "LazyVerticalStaggeredGrid",
    "LazyHorizontalStaggeredGrid",
];
const DRAWING: &[&str] = &["onDraw", "dispatchDraw", "onMeasure", "onLayout", "draw"];
const BINDING: &[&str] = &[
    "onBindViewHolder",
    "onCreateViewHolder",
    "getView",
    "bindView",
];

pub fn analyze(
    project: &Project,
    changes: &[FileChange],
    old: &HashMap<String, Vec<u8>>,
) -> ImpactReport {
    let mut report = ImpactReport {
        files_changed: changes.len(),
        ..ImpactReport::default()
    };
    let mut seeds: Vec<(DeclId, ChangeKind)> = Vec::new();
    let mut removed: Vec<(String, Decl)> = Vec::new();
    let mut old_indexes: Vec<FileIndex> = Vec::new();

    for change in changes {
        let class = classify(&change.path);
        let other = match class.kind {
            FileKind::Build => Some("build"),
            FileKind::Other => Some("other"),
            _ => None,
        };
        if let Some(kind) = other {
            report.other_files.push(OtherFile {
                path: change.path.clone(),
                status: change.status,
                kind,
            });
            continue;
        }
        let new = (change.status != Status::Deleted)
            .then(|| project.file_of(&change.path))
            .flatten();
        let old_path = change.old_path.as_deref().unwrap_or(&change.path);
        let before = old
            .get(old_path)
            .filter(|_| change.status != Status::Added)
            .map(|bytes| crate::extract(old_path, bytes, &classify(old_path)))
            .unwrap_or_default();
        let after = new.map(|fi| &project.files[fi]);
        let empty = FileIndex::default();
        let after_index = after.unwrap_or(&empty);

        let old_by_key: HashMap<&str, usize> = before
            .decls
            .iter()
            .enumerate()
            .map(|(i, d)| (d.key.as_str(), i))
            .collect();
        let new_keys: HashSet<&str> = after_index.decls.iter().map(|d| d.key.as_str()).collect();
        let mut any = false;
        for (di, d) in after_index.decls.iter().enumerate() {
            let id = DeclId(new.unwrap_or(0), di);
            let kind = match old_by_key.get(d.key.as_str()) {
                None => ChangeKind::Added,
                Some(&oi) if before.decls[oi].signature != d.signature => ChangeKind::Signature,
                Some(&oi) if before.decls[oi].body != d.body => ChangeKind::Body,
                Some(_) => continue,
            };
            any = true;
            let o = old_by_key.get(d.key.as_str()).map(|&oi| &before.decls[oi]);
            let (b, a) = match (kind, o) {
                (ChangeKind::Signature, Some(o)) if o.params != d.params => {
                    (o.params.clone(), d.params.clone())
                }
                (ChangeKind::Body, Some(o)) if o.value != d.value => {
                    (o.value.clone(), d.value.clone())
                }
                (ChangeKind::Added, _) => (None, d.value.clone()),
                _ => (None, None),
            };
            report.changes.push(Change {
                file: change.path.clone(),
                line: d.line,
                decl: d.display(),
                kind: d.kind,
                change: kind,
                before: b,
                after: a,
            });
            seeds.push((id, kind));
            if let Some(&oi) = old_by_key.get(d.key.as_str()) {
                let edge = edges(project, &before, oi, after_index, di);
                if !edge.added.is_empty() || !edge.removed.is_empty() {
                    report.edges.push(edge);
                }
            }
        }
        for d in &before.decls {
            if !new_keys.contains(d.key.as_str()) {
                any = true;
                report.changes.push(Change {
                    file: change.path.clone(),
                    line: d.line,
                    decl: d.display(),
                    kind: d.kind,
                    change: ChangeKind::Removed,
                    before: d.value.clone(),
                    after: None,
                });
                removed.push((change.path.clone(), d.clone()));
            }
        }
        if !any && change.status != Status::Deleted {
            if class.kind == FileKind::Asset {
                report.other_files.push(OtherFile {
                    path: change.path.clone(),
                    status: change.status,
                    kind: "asset",
                });
            } else {
                report.cosmetic.push(change.path.clone());
            }
        }
        if after_index.errors > 0 {
            report.limits.push(format!(
                "{}: {} syntax error{}; what it declares and uses may be incomplete",
                change.path,
                after_index.errors,
                if after_index.errors == 1 { "" } else { "s" }
            ));
        }
        old_indexes.push(before);
    }

    let walk = Walk::new(project);
    let impact = walk.propagate(&seeds);
    report.screens = walk.screens(&impact);
    report.callers = callers(project, &seeds);
    report.dangling = dangling(project, &removed);
    let (verify, decls) = verify(project, &walk, &impact, &seeds, &report);
    report.verify = verify;
    report.compat.decls = decls;
    report.limits.push(
        "syntax only: reflection, dependency injection, generated code and routes built at run time \
         are not followed"
            .into(),
    );
    report
}

/// What a changed declaration references, before vs. after.
fn edges(
    project: &Project,
    before: &FileIndex,
    oi: usize,
    after: &FileIndex,
    ni: usize,
) -> EdgeChange {
    let labels = |f: &FileIndex, i: usize| -> BTreeSet<String> {
        f.refs
            .iter()
            .filter(|r| r.from == Some(i))
            .filter_map(|r| edge_label(project, r))
            .collect()
    };
    let old = labels(before, oi);
    let new = labels(after, ni);
    EdgeChange {
        decl: after.decls[ni].display(),
        added: new.difference(&old).cloned().collect(),
        removed: old.difference(&new).cloned().collect(),
    }
}

fn edge_label(project: &Project, r: &Ref) -> Option<String> {
    let qualified = |suffix: &str| match &r.receiver {
        Receiver::Type(t) => format!("{t}.{}{suffix}", r.name),
        _ => format!("{}{suffix}", r.name),
    };
    match &r.kind {
        RefKind::Call => Some(qualified("()")),
        RefKind::ClassLiteral => Some(format!("{}::class", r.name)),
        RefKind::Resource(t) => Some(format!("@{t}/{}", r.name)),
        RefKind::Literal => Some(format!("\"{}\"", r.name)),
        RefKind::Name if r.receiver != Receiver::Unknown && !project.named(&r.name).is_empty() => {
            Some(qualified(""))
        }
        RefKind::Name | RefKind::Type => None,
    }
}

/// A screen reached from a change.
struct Hit {
    screen: DeclId,
    seed: DeclId,
    kind: ChangeKind,
    /// From the change to the declaration in the screen.
    path: Vec<DeclId>,
    confidence: Confidence,
}

struct Impact {
    hits: Vec<Hit>,
    /// Test classes using reached code.
    tests: BTreeSet<String>,
}

struct Walk<'p> {
    project: &'p Project,
    kinds: HashMap<DeclId, Option<ScreenKind>>,
    /// Launcher activities.
    launchers: Vec<DeclId>,
    deep_links: HashMap<String, Vec<String>>,
    /// Screen → (next screen, trigger label).
    graph: HashMap<DeclId, Vec<(DeclId, Option<String>)>>,
}

impl<'p> Walk<'p> {
    fn new(project: &'p Project) -> Self {
        let mut w = Walk {
            project,
            kinds: HashMap::new(),
            launchers: Vec::new(),
            deep_links: HashMap::new(),
            graph: HashMap::new(),
        };
        for (fi, f) in project.files.iter().enumerate() {
            for (di, _) in f.decls.iter().enumerate() {
                let id = DeclId(fi, di);
                let kind = w.classify(id);
                w.kinds.insert(id, kind);
            }
        }
        for f in &project.files {
            for c in &f.components {
                if c.launcher {
                    w.launchers.extend(project.types_named(&c.class));
                }
                if !c.deep_links.is_empty() {
                    w.deep_links
                        .entry(c.class.clone())
                        .or_default()
                        .extend(c.deep_links.iter().cloned());
                }
            }
        }
        w.graph = w.navigation_graph();
        w
    }

    fn kind(&self, id: DeclId) -> Option<ScreenKind> {
        self.kinds.get(&id).copied().flatten()
    }

    fn classify(&self, id: DeclId) -> Option<ScreenKind> {
        let p = self.project;
        if p.classes[id.0].test || !p.classes[id.0].kind.is_source() {
            return None;
        }
        let d = p.decl(id);
        if d.kind == DeclKind::Function {
            let named = d.name.ends_with("Screen") || d.name.ends_with("Route");
            let preview = d.annotations.iter().any(|a| a.contains("Preview"));
            return (d.is_composable() && named && !preview).then_some(ScreenKind::Composable);
        }
        if d.kind != DeclKind::Class || d.signature.split(' ').any(|t| t == "abstract") {
            return None;
        }
        let supers = p.supertypes(id);
        if supers.iter().any(|s| s.ends_with("Activity")) {
            Some(ScreenKind::Activity)
        } else if supers.iter().any(|s| s.ends_with("Fragment")) {
            Some(ScreenKind::Fragment)
        } else {
            None
        }
    }

    /// The screen a declaration belongs to: itself, or the activity or fragment declaring it.
    fn own_screen(&self, id: DeclId) -> Option<DeclId> {
        if self.kind(id).is_some() {
            return Some(id);
        }
        let top = self.project.top_type(id)?;
        matches!(
            self.kind(top),
            Some(ScreenKind::Activity | ScreenKind::Fragment)
        )
        .then_some(top)
    }

    /// Breadth-first through users, from each changed declaration on its own, so every change
    /// keeps its own path to each screen.
    fn propagate(&self, seeds: &[(DeclId, ChangeKind)]) -> Impact {
        let mut impact = Impact {
            hits: Vec::new(),
            tests: BTreeSet::new(),
        };
        for &(seed, kind) in seeds {
            self.propagate_one(seed, kind, &mut impact);
        }
        impact
    }

    fn propagate_one(&self, seed: DeclId, kind: ChangeKind, impact: &mut Impact) {
        let p = self.project;
        // Reached declaration → (reached from, confidence so far, depth).
        let mut prev: HashMap<DeclId, (Option<DeclId>, Confidence, usize)> =
            HashMap::from([(seed, (None, Confidence::Exact, 0))]);
        let mut queue = VecDeque::from([seed]);
        while let Some(n) = queue.pop_front() {
            let (_, confidence, depth) = prev[&n];
            if let Some(screen) = self.own_screen(n) {
                let mut path = vec![n];
                let mut cur = n;
                while let Some(&(Some(before), _, _)) = prev.get(&cur) {
                    path.push(before);
                    cur = before;
                }
                path.reverse();
                impact.hits.push(Hit {
                    screen,
                    seed,
                    kind,
                    path,
                    confidence,
                });
                if screen == n && self.kind(n) != Some(ScreenKind::Composable) {
                    continue;
                }
            }
            if depth >= MAX_DEPTH {
                continue;
            }
            let mut next: Vec<(DeclId, Confidence)> = Vec::new();
            for &(fi, ri, c) in p.users(n) {
                let Some(from) = p.files[fi].refs[ri].from else {
                    continue;
                };
                let from = DeclId(fi, from);
                if p.classes[fi].test {
                    if let Some(t) = p.top_type(from) {
                        impact.tests.insert(p.decl(t).name.clone());
                    }
                    continue;
                }
                if c == Confidence::Ambiguous && depth > 0 {
                    continue;
                }
                next.push((from, confidence.min(c)));
            }
            let d = p.decl(n);
            // Subclasses inherit what changed in a type.
            let owner_type = if d.kind.is_type() {
                Some(n)
            } else {
                p.enclosing_type(n)
            };
            if let Some(t) = owner_type {
                for &s in p.subtypes(&p.decl(t).name) {
                    next.push((s, confidence.min(Confidence::Likely)));
                }
            }
            // An override is called through the declaration it overrides (an interface injected
            // somewhere, a base class).
            if d.overrides
                && let Some(t) = p.enclosing_type(n)
            {
                let supers = p.supertypes(t);
                for &m in p.named(&d.name) {
                    let o = p.decl(m);
                    if o.kind == d.kind
                        && supers
                            .iter()
                            .any(|s| o.owner.rsplit('.').next() == Some(s.as_str()))
                    {
                        next.push((m, confidence.min(Confidence::Likely)));
                    }
                }
            }
            // A changed manifest entry changes how its component runs.
            if d.kind == DeclKind::Manifest {
                for t in p.types_named(&d.name) {
                    next.push((t, confidence));
                }
            }
            for (m, c) in next {
                if let std::collections::hash_map::Entry::Vacant(e) = prev.entry(m) {
                    e.insert((Some(n), c, depth + 1));
                    queue.push_back(m);
                }
            }
        }
    }

    /// An activity reached through one of its composable screens: that screen's host, reported
    /// with the screen rather than on its own.
    fn through_composable_screen(&self, hit: &Hit) -> bool {
        self.kind(hit.screen) != Some(ScreenKind::Composable)
            && hit
                .path
                .iter()
                .any(|&n| n != hit.screen && self.kind(n) == Some(ScreenKind::Composable))
    }

    fn screens(&self, impact: &Impact) -> Vec<ScreenImpact> {
        let p = self.project;
        let severity = |k: ChangeKind| match k {
            ChangeKind::Signature => 3,
            ChangeKind::Removed => 2,
            ChangeKind::Body => 1,
            ChangeKind::Added => 0,
        };
        let mut best: HashMap<DeclId, (&Hit, HashSet<DeclId>)> = HashMap::new();
        for hit in &impact.hits {
            if self.through_composable_screen(hit) {
                continue;
            }
            let rank = |h: &Hit| {
                (
                    h.confidence,
                    severity(h.kind),
                    std::cmp::Reverse(h.path.len()),
                )
            };
            let entry = best
                .entry(hit.screen)
                .or_insert_with(|| (hit, HashSet::new()));
            entry.1.insert(hit.seed);
            if rank(hit) > rank(entry.0) {
                entry.0 = hit;
            }
        }
        let best: HashMap<DeclId, (Confidence, Vec<DeclId>, usize)> = best
            .into_iter()
            .map(|(screen, (hit, seeds))| (screen, (hit.confidence, hit.path.clone(), seeds.len())))
            .collect();
        let mut screens: Vec<ScreenImpact> = best
            .into_iter()
            .map(|(screen, (confidence, path, changes))| {
                let d = p.decl(screen);
                let mut via: Vec<String> = path.iter().map(|&n| p.decl(n).display()).collect();
                via.dedup();
                if via.last().is_some_and(|l| *l == d.display()) && via.len() > 1 {
                    via.pop();
                }
                let kind = self.kind(screen).unwrap_or(ScreenKind::Activity);
                let host = (kind == ScreenKind::Composable)
                    .then(|| self.host(screen))
                    .flatten();
                let target = host.unwrap_or(screen);
                ScreenImpact {
                    screen: d.name.clone(),
                    kind,
                    file: p.files[screen.0].path.clone(),
                    via,
                    confidence,
                    host: host.map(|h| p.decl(h).name.clone()),
                    changes,
                    reach: self.reach(target),
                }
            })
            .collect();
        screens.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then(a.via.len().cmp(&b.via.len()))
                .then(a.screen.cmp(&b.screen))
        });
        // Overloads of one composable are one screen.
        let mut seen = HashSet::new();
        screens.retain(|s| seen.insert((s.screen.clone(), s.file.clone())));
        screens
    }

    /// The activity that shows a composable, following its callers.
    fn host(&self, composable: DeclId) -> Option<DeclId> {
        self.screens_above(composable, MAX_DEPTH)
            .into_iter()
            .find(|&s| self.kind(s) == Some(ScreenKind::Activity))
    }

    /// Activities and fragments a declaration is used from, nearest first.
    fn screens_above(&self, start: DeclId, max_depth: usize) -> Vec<DeclId> {
        let p = self.project;
        let mut found = Vec::new();
        let mut seen = HashSet::from([start]);
        let mut queue = VecDeque::from([(start, 0)]);
        while let Some((n, depth)) = queue.pop_front() {
            if let Some(s) = self.own_screen(n)
                && matches!(
                    self.kind(s),
                    Some(ScreenKind::Activity | ScreenKind::Fragment)
                )
            {
                if !found.contains(&s) {
                    found.push(s);
                }
                continue;
            }
            if depth >= max_depth {
                continue;
            }
            for &(fi, ri, c) in p.users(n) {
                if c == Confidence::Ambiguous || p.classes[fi].test {
                    continue;
                }
                if let Some(from) = p.files[fi].refs[ri].from {
                    let m = DeclId(fi, from);
                    if seen.insert(m) {
                        queue.push_back((m, depth + 1));
                    }
                }
            }
        }
        found
    }

    /// Screen-to-screen edges: class literals of screens (intents), fragment constructors and
    /// fragments named in layouts, navigation-graph actions.
    fn navigation_graph(&self) -> HashMap<DeclId, Vec<(DeclId, Option<String>)>> {
        let p = self.project;
        let labels = self.labels();
        let mut graph: HashMap<DeclId, Vec<(DeclId, Option<String>)>> = HashMap::new();
        for (fi, f) in p.files.iter().enumerate() {
            if p.classes[fi].test {
                continue;
            }
            for r in &f.refs {
                let fragment_use = matches!(r.kind, RefKind::Call | RefKind::Type);
                if !(r.kind == RefKind::ClassLiteral || fragment_use) {
                    continue;
                }
                let Some(from) = r.from else { continue };
                let (targets, c) = p.resolve(fi, r);
                if c == Confidence::Ambiguous {
                    continue;
                }
                for t in targets {
                    let kind = self.kind(t);
                    let ok = match r.kind {
                        RefKind::ClassLiteral => {
                            matches!(kind, Some(ScreenKind::Activity | ScreenKind::Fragment))
                        }
                        _ => kind == Some(ScreenKind::Fragment),
                    };
                    if !ok {
                        continue;
                    }
                    let trigger = r
                        .trigger
                        .as_ref()
                        .map(|id| labels.get(id).cloned().unwrap_or_else(|| id.clone()));
                    for s in self.screens_above(DeclId(fi, from), SOURCE_DEPTH) {
                        if s != t {
                            graph.entry(s).or_default().push((t, trigger.clone()));
                        }
                    }
                }
            }
            for e in &f.edges {
                for a in p.types_named(&e.from) {
                    for b in p.types_named(&e.to) {
                        graph.entry(a).or_default().push((b, e.trigger.clone()));
                    }
                }
            }
        }
        graph
    }

    /// View id → label shown on screen; string resources resolved to their default value.
    fn labels(&self) -> HashMap<String, String> {
        let p = self.project;
        let mut strings: HashMap<&str, &str> = HashMap::new();
        for (fi, f) in p.files.iter().enumerate() {
            if p.classes[fi].kind == FileKind::Values && p.classes[fi].qualifiers.is_empty() {
                for d in &f.decls {
                    if d.rtype.as_deref() == Some("string")
                        && let Some(v) = &d.value
                    {
                        strings.insert(&d.name, v);
                    }
                }
            }
        }
        let mut labels = HashMap::new();
        for f in &p.files {
            for (id, label) in &f.labels {
                let label = match label.strip_prefix("@string/") {
                    Some(name) => strings.get(name).map_or(label.as_str(), |v| *v),
                    None => label.as_str(),
                };
                labels.insert(id.clone(), format!("\"{label}\""));
            }
        }
        labels
    }

    /// Deep links, then the taps from the launcher screen.
    fn reach(&self, screen: DeclId) -> Vec<String> {
        let p = self.project;
        let name = &p.decl(screen).name;
        let mut out: Vec<String> = self
            .deep_links
            .get(name)
            .map(|l| l.iter().take(2).cloned().collect())
            .unwrap_or_default();
        if self.launchers.contains(&screen) {
            out.push("launcher".into());
            return out;
        }
        let mut prev: HashMap<DeclId, (DeclId, Option<String>)> = HashMap::new();
        let mut queue: VecDeque<DeclId> = self.launchers.iter().copied().collect();
        let mut seen: HashSet<DeclId> = queue.iter().copied().collect();
        while let Some(n) = queue.pop_front() {
            if n == screen {
                let mut steps = vec![p.decl(n).name.clone()];
                let mut cur = n;
                while let Some((from, trigger)) = prev.get(&cur) {
                    if let Some(t) = trigger {
                        steps.push(t.clone());
                    }
                    steps.push(p.decl(*from).name.clone());
                    cur = *from;
                }
                steps.reverse();
                out.push(steps.join(" ▸ "));
                break;
            }
            for (m, trigger) in self.graph.get(&n).into_iter().flatten() {
                if seen.insert(*m) {
                    prev.insert(*m, (n, trigger.clone()));
                    queue.push_back(*m);
                }
            }
        }
        out
    }
}

/// Call sites of declarations whose signature changed, flagging argument counts that no longer fit.
fn callers(project: &Project, seeds: &[(DeclId, ChangeKind)]) -> Vec<Callers> {
    let mut out = Vec::new();
    for &(id, kind) in seeds {
        let d = project.decl(id);
        if kind != ChangeKind::Signature
            || !matches!(d.kind, DeclKind::Function | DeclKind::Class)
            || d.overrides
        {
            continue;
        }
        let mut sites: Vec<Site> = project
            .users(id)
            .iter()
            .filter(|(fi, ri, _)| {
                matches!(
                    project.files[*fi].refs[*ri].kind,
                    RefKind::Call | RefKind::Name
                )
            })
            .map(|&(fi, ri, confidence)| {
                let r = &project.files[fi].refs[ri];
                let note = match (d.arity, r.args) {
                    (Some((min, max)), Some(n)) if n < min || max.is_some_and(|m| n > m) => {
                        let needs = match max {
                            Some(m) if m == min => format!("{min}"),
                            Some(m) => format!("{min}–{m}"),
                            None => format!("{min}+"),
                        };
                        Some(format!(
                            "{n} argument{}, needs {needs}",
                            if n == 1 { "" } else { "s" }
                        ))
                    }
                    _ => None,
                };
                Site {
                    file: project.files[fi].path.clone(),
                    line: r.line,
                    from: r
                        .from
                        .map(|i| project.files[fi].decls[i].display())
                        .unwrap_or_default(),
                    confidence,
                    note,
                }
            })
            .collect();
        if sites.is_empty() {
            continue;
        }
        // Calls that don't fit first: they are what breaks.
        sites.sort_by_key(|s| (s.note.is_none(), s.file.clone(), s.line));
        out.push(Callers {
            decl: d.display(),
            sites,
        });
    }
    out
}

/// Uses of removed declarations that are still there.
fn dangling(project: &Project, removed: &[(String, Decl)]) -> Vec<Callers> {
    let mut out = Vec::new();
    for (path, d) in removed {
        // Still declared elsewhere under the same name and kind: moved, not removed.
        let moved = project.named(&d.name).iter().any(|&id| {
            let o = project.decl(id);
            o.kind == d.kind && o.owner == d.owner && o.rtype == d.rtype
        });
        if moved || d.kind == DeclKind::Manifest || d.overrides {
            continue;
        }
        let owner = d.owner.rsplit('.').next().unwrap_or("");
        let mut sites = Vec::new();
        for (fi, f) in project.files.iter().enumerate() {
            let same_scope =
                f.path == *path || project.files[fi].package == package_of(project, path);
            for r in &f.refs {
                if r.name != d.name {
                    continue;
                }
                let matches = match (&r.kind, d.kind) {
                    (RefKind::Resource(t), DeclKind::Resource) => d.rtype.as_deref() == Some(t),
                    (RefKind::Type | RefKind::ClassLiteral | RefKind::Call, k) if k.is_type() => {
                        same_scope || imports(f, &d.name)
                    }
                    (RefKind::Call | RefKind::Name, DeclKind::Function | DeclKind::Property) => {
                        match &r.receiver {
                            Receiver::Implicit => same_scope || imports(f, &d.name),
                            Receiver::Type(t) => t == owner,
                            Receiver::Unknown => false,
                        }
                    }
                    _ => false,
                };
                if matches && project.resolve(fi, r).0.is_empty() {
                    sites.push(Site {
                        file: f.path.clone(),
                        line: r.line,
                        from: r.from.map(|i| f.decls[i].display()).unwrap_or_default(),
                        confidence: Confidence::Likely,
                        note: None,
                    });
                }
            }
        }
        if !sites.is_empty() {
            out.push(Callers {
                decl: d.display(),
                sites,
            });
        }
    }
    out
}

fn package_of(project: &Project, path: &str) -> String {
    project
        .file_of(path)
        .map(|fi| project.files[fi].package.clone())
        .unwrap_or_default()
}

fn imports(f: &FileIndex, name: &str) -> bool {
    f.imports.iter().any(|i| i.rsplit('.').next() == Some(name))
}

fn verify(
    project: &Project,
    walk: &Walk,
    impact: &Impact,
    seeds: &[(DeclId, ChangeKind)],
    report: &ImpactReport,
) -> (Verify, Vec<DeclFacts>) {
    let p = project;
    let mut v = Verify::default();
    let mut facts = Vec::new();
    for s in &report.screens {
        let name = match &s.host {
            Some(h) => format!("{} (in {h})", s.screen),
            None => s.screen.clone(),
        };
        if !v.functional.contains(&name) {
            v.functional.push(name);
        }
    }
    // Screens each change reaches.
    let mut reached: HashMap<DeclId, BTreeSet<String>> = HashMap::new();
    for hit in impact
        .hits
        .iter()
        .filter(|h| !walk.through_composable_screen(h))
    {
        let screen = p.decl(hit.screen).name.clone();
        reached.entry(hit.seed).or_default().insert(screen);
    }
    let on = |id: DeclId| -> String {
        match reached.get(&id) {
            Some(s) if !s.is_empty() => {
                format!(" → {}", s.iter().cloned().collect::<Vec<_>>().join(", "))
            }
            _ => String::new(),
        }
    };
    let launchers: HashSet<DeclId> = walk.launchers.iter().copied().collect();
    for &(id, kind) in seeds {
        let d = p.decl(id);
        let class = &p.classes[id.0];
        if class.test {
            continue;
        }
        let refs: Vec<&Ref> = p.files[id.0]
            .refs
            .iter()
            .filter(|r| r.from == Some(id.1))
            .collect();
        let owner = p.enclosing_type(id);
        let owner_supers = owner.map(|o| p.supertypes(o)).unwrap_or_default();

        let display = d.display();
        let change = report
            .changes
            .iter()
            .find(|c| c.file == p.files[id.0].path && c.decl == display);
        let mut uses: Vec<String> = Vec::new();
        for r in &refs {
            if matches!(r.kind, RefKind::Literal | RefKind::Resource(_)) {
                continue;
            }
            if let Receiver::Type(t) = &r.receiver {
                uses.push(format!("{t}.{}", r.name));
            }
            uses.push(r.name.clone());
        }
        uses.sort();
        uses.dedup();
        let mut supertypes = p.supertypes(id);
        supertypes.extend(owner_supers.iter().cloned());
        facts.push(DeclFacts {
            decl: display,
            file: p.files[id.0].path.clone(),
            line: d.line,
            kind: d.kind,
            change: kind,
            rtype: d.rtype.clone(),
            qualifiers: class.qualifiers.clone(),
            before: change.and_then(|c| c.before.clone()),
            after: change
                .and_then(|c| c.after.clone())
                .or_else(|| d.value.clone()),
            annotations: d.annotations.clone(),
            supertypes,
            uses,
            api_levels: d.api_levels.clone(),
            screens: reached
                .get(&id)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default(),
        });

        let visual = match d.kind {
            DeclKind::Resource => {
                !matches!(d.rtype.as_deref(), Some("id" | "asset" | "raw" | "xml"))
            }
            DeclKind::Function => d.is_composable(),
            DeclKind::Class => p
                .supertypes(id)
                .iter()
                .any(|s| s.ends_with("View") || s.ends_with("Layout")),
            _ => false,
        };
        if visual {
            v.ui.push(format!("{}{}", d.display(), on(id)));
        }

        let perf = if BINDING.contains(&d.name.as_str())
            || owner_supers.iter().any(|s| s.ends_with("Adapter"))
        {
            Some("list binding: scrolling")
        } else if DRAWING.contains(&d.name.as_str()) {
            Some("drawing and layout passes")
        } else if d.is_composable() && refs.iter().any(|r| LAZY_LISTS.contains(&r.name.as_str())) {
            Some("lazy list: scrolling")
        } else if owner_supers.iter().any(|s| s == "Application")
            || owner.is_some_and(|o| launchers.contains(&o)) && d.name == "onCreate"
        {
            Some("app startup")
        } else {
            None
        };
        if let Some(reason) = perf {
            v.performance.push(format!("{}: {reason}", d.display()));
        }

        if d.kind == DeclKind::Manifest {
            v.compatibility.push(format!(
                "{} ({}): behavior can differ by Android version",
                d.display(),
                kind_word(kind)
            ));
        }
        if !class.qualifiers.is_empty() && d.kind == DeclKind::Resource {
            v.compatibility.push(format!(
                "{} in {}-{}: only on devices matching `{}`",
                d.display(),
                class.res_dir.as_deref().unwrap_or("res"),
                class.qualifiers,
                class.qualifiers
            ));
        }
        if refs
            .iter()
            .any(|r| r.name == "SDK_INT" || r.name == "VERSION_CODES")
            || d.annotations
                .iter()
                .any(|a| a == "RequiresApi" || a == "TargetApi")
        {
            v.compatibility.push(format!(
                "{}: depends on the API level; check the lowest and newest supported",
                d.display()
            ));
        }
        if d.rtype.as_deref() == Some("string") && class.qualifiers.is_empty() {
            let translations = translations(p, &d.name);
            match kind {
                ChangeKind::Body if !translations.with.is_empty() => v.compatibility.push(format!(
                    "{} changed; translations may be stale: {}",
                    d.display(),
                    translations.with.join(", ")
                )),
                ChangeKind::Added if !translations.without.is_empty() => {
                    v.compatibility.push(format!(
                        "{} added; not translated in {}",
                        d.display(),
                        translations.without.join(", ")
                    ))
                }
                _ => {}
            }
        }
    }
    v.tests = impact.tests.iter().cloned().collect();
    (v, facts)
}

fn kind_word(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Signature | ChangeKind::Body => "changed",
    }
}

struct Translations {
    /// Qualified `values-*` directories that translate the string.
    with: Vec<String>,
    /// Qualified directories with strings of their own that lack it.
    without: Vec<String>,
}

fn translations(p: &Project, name: &str) -> Translations {
    let mut with = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    for (fi, f) in p.files.iter().enumerate() {
        let c = &p.classes[fi];
        if c.kind != FileKind::Values || c.qualifiers.is_empty() {
            continue;
        }
        let dir = format!("values-{}", c.qualifiers);
        if f.decls.iter().any(|d| d.rtype.as_deref() == Some("string")) {
            dirs.insert(dir.clone());
        }
        if f.decls
            .iter()
            .any(|d| d.rtype.as_deref() == Some("string") && d.name == name)
        {
            with.insert(dir);
        }
    }
    Translations {
        without: dirs.difference(&with).cloned().collect(),
        with: with.into_iter().collect(),
    }
}
