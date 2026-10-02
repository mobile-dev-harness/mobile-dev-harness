//! Declarations and references in Java sources (`tree-sitter-java`).

use std::collections::HashMap;

use tree_sitter::Node;

use crate::model::{Decl, DeclKind, FileIndex, Receiver, Ref, RefKind};
use crate::source::{RESOURCE_TYPES, binding_layout, literal_file_name};
use crate::syntax::{self, child_of_kind, line, named_children, text};

const MEMBERS: &[&str] = &[
    "method_declaration",
    "constructor_declaration",
    "field_declaration",
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
    "enum_constant",
];
const TYPES: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

pub fn extract(path: &str, src: &str) -> FileIndex {
    let language = tree_sitter_java::LANGUAGE.into();
    let mut out = FileIndex {
        path: path.to_owned(),
        ..FileIndex::default()
    };
    let Some((tree, errors)) = syntax::parse(&language, src, false) else {
        out.errors = 1;
        return out;
    };
    out.errors = errors;
    let mut x = Extractor {
        src,
        out,
        stack: Vec::new(),
        owners: Vec::new(),
        var_types: HashMap::new(),
        var_ids: HashMap::new(),
    };
    x.collect_vars(tree.root_node());
    x.visit(tree.root_node());
    let mut out = x.out;
    crate::source::disambiguate_overloads(&mut out.decls);
    out
}

struct Extractor<'s> {
    src: &'s str,
    out: FileIndex,
    stack: Vec<usize>,
    owners: Vec<String>,
    var_types: HashMap<String, String>,
    var_ids: HashMap<String, String>,
}

impl<'s> Extractor<'s> {
    fn text(&self, node: Node) -> &'s str {
        text(node, self.src)
    }

    fn from(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    fn local(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|&i| !self.out.decls[i].kind.is_type())
    }

    fn push_ref(&mut self, kind: RefKind, name: &str, receiver: Receiver, node: Node) -> usize {
        if matches!(kind, RefKind::Type | RefKind::Name)
            && let Some(layout) = binding_layout(name)
        {
            self.out.refs.push(Ref {
                kind: RefKind::Resource("layout".into()),
                name: layout,
                receiver: Receiver::Implicit,
                line: line(node),
                from: self.from(),
                args: None,
                trigger: None,
            });
        }
        self.out.refs.push(Ref {
            kind,
            name: name.to_owned(),
            receiver,
            line: line(node),
            from: self.from(),
            args: None,
            trigger: None,
        });
        self.out.refs.len() - 1
    }

    fn visit(&mut self, node: Node) {
        match node.kind() {
            "package_declaration" => {
                if let Some(q) = named_children(node).first() {
                    self.out.package = self.text(*q).to_owned();
                }
            }
            "import_declaration" => {
                let kids = named_children(node);
                if let Some(q) = kids
                    .iter()
                    .find(|c| matches!(c.kind(), "scoped_identifier" | "identifier"))
                {
                    let mut name = self.text(*q).to_owned();
                    if kids.iter().any(|c| c.kind() == "asterisk") {
                        name.push_str(".*");
                    }
                    self.out.imports.push(name);
                }
            }
            k if TYPES.contains(&k) => self.type_decl(node),
            "method_declaration" | "constructor_declaration" => self.method(node),
            "field_declaration" => self.field(node),
            "enum_constant" => self.enum_constant(node),
            "method_invocation" => self.invocation(node),
            "object_creation_expression" => {
                if let Some(t) = node.child_by_field_name("type")
                    && let Some(name) = type_name(t, self.src)
                {
                    let i = self.push_ref(RefKind::Call, &name, Receiver::Implicit, t);
                    self.out.refs[i].args = node
                        .child_by_field_name("arguments")
                        .map(|a| named_children(a).len());
                }
                for c in named_children(node) {
                    if Some(c) != node.child_by_field_name("type") {
                        self.visit(c);
                    }
                }
            }
            "class_literal" => {
                if let Some(t) = named_children(node).first()
                    && let Some(name) = type_name(*t, self.src)
                {
                    let trigger = self.trigger(node);
                    let i = self.push_ref(RefKind::ClassLiteral, &name, Receiver::Implicit, node);
                    self.out.refs[i].trigger = trigger;
                }
            }
            "field_access" => self.field_access(node),
            "type_identifier" => {
                let name = self.text(node);
                self.push_ref(RefKind::Type, name, Receiver::Implicit, node);
            }
            "scoped_type_identifier" => {
                if let Some(last) = named_children(node).last() {
                    let name = self.text(*last);
                    self.push_ref(RefKind::Type, name, Receiver::Implicit, *last);
                }
            }
            "identifier" => {
                let name = self.text(node);
                self.push_ref(RefKind::Name, name, Receiver::Implicit, node);
            }
            "variable_declarator"
            | "formal_parameter"
            | "catch_formal_parameter"
            | "spread_parameter" => {
                let name = node.child_by_field_name("name");
                for c in named_children(node) {
                    if Some(c) != name {
                        self.visit(c);
                    }
                }
            }
            "lambda_expression" => {
                let params = node.child_by_field_name("parameters");
                for c in named_children(node) {
                    if Some(c) == params {
                        // Parameter names are declarations; typed parameters still reference types.
                        if c.kind() == "formal_parameters" {
                            self.visit(c);
                        }
                    } else {
                        self.visit(c);
                    }
                }
            }
            "string_fragment" => {
                if let Some(file) = literal_file_name(self.text(node)) {
                    self.push_ref(RefKind::Literal, &file, Receiver::Implicit, node);
                }
            }
            "labeled_statement" | "break_statement" | "continue_statement" => {
                for c in named_children(node) {
                    if c.kind() != "identifier" {
                        self.visit(c);
                    }
                }
            }
            "line_comment" | "block_comment" | "this" | "super" => {}
            _ => {
                for c in named_children(node) {
                    self.visit(c);
                }
            }
        }
    }

    fn owner(&self) -> String {
        self.owners.join(".")
    }

    fn modifiers(&self, node: Node, decl: &mut Decl) {
        let Some(m) = child_of_kind(node, "modifiers") else {
            return;
        };
        for c in named_children(m) {
            if matches!(c.kind(), "marker_annotation" | "annotation")
                && let Some(name) = c.child_by_field_name("name")
            {
                let name = self
                    .text(name)
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                if name == "Override" {
                    decl.overrides = true;
                }
                decl.annotations.push(name);
            }
        }
    }

    fn open(&mut self, decl: Decl) {
        self.out.decls.push(decl);
        self.stack.push(self.out.decls.len() - 1);
    }

    fn visit_except(&mut self, node: Node, skip: Option<Node>) {
        for c in named_children(node) {
            if Some(c) != skip {
                self.visit(c);
            }
        }
    }

    fn type_decl(&mut self, node: Node) {
        let name_node = node.child_by_field_name("name");
        let Some(name) = name_node.map(|n| self.text(n).to_owned()) else {
            return self.visit_except(node, None);
        };
        let kind = match node.kind() {
            "interface_declaration" | "annotation_type_declaration" => DeclKind::Interface,
            "enum_declaration" => DeclKind::Enum,
            _ => DeclKind::Class,
        };
        let mut decl = Decl::new(kind, &name, self.owner(), line(node));
        self.modifiers(node, &mut decl);
        for field in ["superclass", "interfaces", "extends_interfaces"] {
            if let Some(s) = node.child_by_field_name(field) {
                collect_type_names(s, self.src, &mut decl.supertypes);
            }
        }
        if node.kind() == "record_declaration"
            && let Some(p) = node.child_by_field_name("parameters")
        {
            decl.params = Some(syntax::collapsed(self.text(p)));
            decl.arity = Some(arity(p));
        }
        let body = node.child_by_field_name("body");
        decl.signature = norm(node, self.src, &|n| Some(n) == body);
        decl.body = body.map_or(0, |b| {
            syntax::token_hash(b, self.src, &|n| MEMBERS.contains(&n.kind()))
        });
        if self.local() {
            return self.visit_except(node, name_node);
        }
        self.open(decl);
        self.owners.push(name);
        self.visit_except(node, name_node);
        self.owners.pop();
        self.stack.pop();
    }

    fn method(&mut self, node: Node) {
        let name_node = node.child_by_field_name("name");
        let (Some(name_node), false) = (name_node, self.local()) else {
            return self.visit_except(node, name_node);
        };
        let constructor = node.kind() == "constructor_declaration";
        let mut decl = Decl::new(
            if constructor {
                DeclKind::Constructor
            } else {
                DeclKind::Function
            },
            self.text(name_node),
            self.owner(),
            line(name_node),
        );
        if constructor {
            decl.key = format!("{}.<init>", self.owner());
        }
        self.modifiers(node, &mut decl);
        if let Some(p) = node.child_by_field_name("parameters") {
            decl.params = Some(syntax::collapsed(self.text(p)));
            decl.arity = Some(arity(p));
        }
        let body = node.child_by_field_name("body");
        decl.signature = norm(node, self.src, &|n| Some(n) == body);
        decl.body = body.map_or(0, |b| syntax::token_hash(b, self.src, &|_| false));
        self.open(decl);
        self.visit_except(node, Some(name_node));
        self.stack.pop();
    }

    fn field(&mut self, node: Node) {
        if self.local() {
            return self.visit_except(node, None);
        }
        let mut head = String::new();
        for c in named_children(node) {
            if c.kind() != "variable_declarator" {
                syntax::tokens(c, self.src, &|_| false, &mut head);
            }
        }
        for declarator in named_children(node)
            .into_iter()
            .filter(|c| c.kind() == "variable_declarator")
        {
            let Some(name) = declarator.child_by_field_name("name") else {
                continue;
            };
            let mut decl = Decl::new(
                DeclKind::Property,
                self.text(name),
                self.owner(),
                line(name),
            );
            self.modifiers(node, &mut decl);
            decl.signature = format!("{head} {}", self.text(name));
            decl.body = declarator
                .child_by_field_name("value")
                .map_or(0, |v| syntax::token_hash(v, self.src, &|_| false));
            self.open(decl);
            if let Some(t) = node.child_by_field_name("type") {
                self.visit(t);
            }
            self.visit(declarator);
            self.stack.pop();
        }
    }

    fn enum_constant(&mut self, node: Node) {
        let name_node = node.child_by_field_name("name");
        let Some(name) = name_node.map(|n| self.text(n).to_owned()) else {
            return self.visit_except(node, None);
        };
        let mut decl = Decl::new(DeclKind::Property, &name, self.owner(), line(node));
        decl.signature = name;
        decl.body = syntax::token_hash(node, self.src, &|_| false);
        self.open(decl);
        self.visit_except(node, name_node);
        self.stack.pop();
    }

    fn invocation(&mut self, node: Node) {
        let object = node.child_by_field_name("object");
        let receiver = object.map_or(Receiver::Implicit, |o| self.receiver_of(o));
        if let Some(name) = node.child_by_field_name("name") {
            let n = self.text(name);
            let i = self.push_ref(RefKind::Call, n, receiver, name);
            self.out.refs[i].args = node
                .child_by_field_name("arguments")
                .map(|a| named_children(a).len());
        }
        let name = node.child_by_field_name("name");
        for c in named_children(node) {
            if Some(c) != name {
                self.visit(c);
            }
        }
    }

    /// `R.layout.main` → (`layout`, `main`); `android.R.…` → empty.
    fn resource(&self, node: Node) -> Option<(String, String)> {
        let segments = chain(node, self.src)?;
        let n = segments.len();
        if n < 3 || segments[n - 3] != "R" || !RESOURCE_TYPES.contains(&segments[n - 2]) {
            return None;
        }
        if n >= 4 && segments[n - 4] == "android" {
            return Some((String::new(), String::new()));
        }
        Some((segments[n - 2].to_owned(), segments[n - 1].to_owned()))
    }

    fn field_access(&mut self, node: Node) {
        if let Some((rtype, name)) = self.resource(node) {
            if !rtype.is_empty() {
                self.push_ref(RefKind::Resource(rtype), &name, Receiver::Implicit, node);
            }
            return;
        }
        let object = node.child_by_field_name("object");
        if let Some(field) = node.child_by_field_name("field") {
            let receiver = object.map_or(Receiver::Implicit, |o| self.receiver_of(o));
            let name = self.text(field);
            self.push_ref(RefKind::Name, name, receiver, field);
        }
        if let Some(o) = object {
            self.visit(o);
        }
    }

    fn receiver_of(&self, node: Node) -> Receiver {
        match node.kind() {
            "identifier" => {
                let name = self.text(node);
                match self.var_types.get(name) {
                    Some(t) => Receiver::Type(t.clone()),
                    None if name.starts_with(char::is_uppercase) => Receiver::Type(name.to_owned()),
                    None => Receiver::Unknown,
                }
            }
            "this" | "super" => Receiver::Implicit,
            "object_creation_expression" => node
                .child_by_field_name("type")
                .and_then(|t| type_name(t, self.src))
                .map_or(Receiver::Unknown, Receiver::Type),
            "field_access" => match chain(node, self.src) {
                Some(s) if s.last().is_some_and(|l| l.starts_with(char::is_uppercase)) => {
                    Receiver::Type(s.last().unwrap().to_string())
                }
                _ => Receiver::Unknown,
            },
            _ => Receiver::Unknown,
        }
    }

    fn trigger(&self, node: Node) -> Option<String> {
        let mut current = node;
        let mut level = 0;
        while let Some(parent) = current.parent() {
            level += 1;
            let member_of_named_type = |n: Node| {
                n.parent()
                    .and_then(|body| body.parent())
                    .is_some_and(|t| TYPES.contains(&t.kind()))
            };
            if parent.kind() == "program"
                || (matches!(parent.kind(), "method_declaration" | "field_declaration")
                    && member_of_named_type(parent))
            {
                break;
            }
            if parent.kind() == "method_invocation"
                && let Some(name) = parent.child_by_field_name("name")
                && is_click_listener(self.text(name))
                && let Some(object) = parent.child_by_field_name("object")
            {
                if let Some(id) = self.view_id_in(object, 20) {
                    return Some(id);
                }
                if object.kind() == "identifier"
                    && let Some(id) = self.var_ids.get(self.text(object))
                {
                    return Some(id.clone());
                }
            }
            if level <= 3
                && let Some(id) = self.view_id_in(parent, 40)
            {
                return Some(id);
            }
            current = parent;
        }
        None
    }

    fn view_id_in(&self, node: Node, budget: usize) -> Option<String> {
        let mut stack = vec![node];
        let mut seen = 0;
        while let Some(n) = stack.pop() {
            seen += 1;
            if seen > budget {
                return None;
            }
            if n.kind() == "field_access"
                && let Some((rtype, name)) = self.resource(n)
                && rtype == "id"
            {
                return Some(name);
            }
            stack.extend(named_children(n).into_iter().rev());
        }
        None
    }

    fn collect_vars(&mut self, node: Node) {
        match node.kind() {
            "local_variable_declaration" | "field_declaration" => {
                let t = node
                    .child_by_field_name("type")
                    .and_then(|t| type_name(t, self.src));
                for d in named_children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "variable_declarator")
                {
                    let Some(name) = d.child_by_field_name("name") else {
                        continue;
                    };
                    let name = self.text(name).to_owned();
                    if let Some(t) = &t {
                        self.var_types.insert(name.clone(), t.clone());
                    }
                    if let Some(v) = d.child_by_field_name("value")
                        && let Some(id) = self.view_id_in(v, 30)
                    {
                        self.var_ids.insert(name, id);
                    }
                }
            }
            "formal_parameter" => {
                if let (Some(t), Some(name)) = (
                    node.child_by_field_name("type"),
                    node.child_by_field_name("name"),
                ) && let Some(t) = type_name(t, self.src)
                {
                    self.var_types.insert(self.text(name).to_owned(), t);
                }
            }
            _ => {}
        }
        for c in named_children(node) {
            self.collect_vars(c);
        }
    }
}

fn is_click_listener(name: &str) -> bool {
    name.starts_with("setOn") && name.ends_with("Listener")
}

fn norm(node: Node, src: &str, skip: &dyn Fn(Node) -> bool) -> String {
    let mut s = String::new();
    syntax::tokens(node, src, skip, &mut s);
    s
}

fn arity(params: Node) -> (usize, Option<usize>) {
    let kids = named_children(params);
    let vararg = kids.iter().any(|k| k.kind() == "spread_parameter");
    let fixed = kids
        .iter()
        .filter(|k| matches!(k.kind(), "formal_parameter" | "receiver_parameter"))
        .count();
    (fixed, (!vararg).then_some(fixed))
}

/// Simple name of a type: `Foo`, `a.b.Foo` → `Foo`, `List<Foo>` → `List`.
fn type_name(node: Node, src: &str) -> Option<String> {
    match node.kind() {
        "type_identifier" => Some(text(node, src).to_owned()),
        "scoped_type_identifier" => named_children(node)
            .last()
            .map(|n| text(*n, src).to_owned()),
        "generic_type" => named_children(node)
            .first()
            .and_then(|n| type_name(*n, src)),
        _ => None,
    }
}

fn collect_type_names(node: Node, src: &str, out: &mut Vec<String>) {
    match node.kind() {
        "type_identifier" | "scoped_type_identifier" => out.extend(type_name(node, src)),
        "generic_type" => out.extend(type_name(node, src)),
        _ => {
            for c in named_children(node) {
                collect_type_names(c, src, out);
            }
        }
    }
}

/// Identifiers of a pure `a.b.C` field-access chain.
fn chain<'s>(node: Node, src: &'s str) -> Option<Vec<&'s str>> {
    match node.kind() {
        "identifier" => Some(vec![text(node, src)]),
        "field_access" => {
            let mut s = chain(node.child_by_field_name("object")?, src)?;
            s.push(text(node.child_by_field_name("field")?, src));
            Some(s)
        }
        _ => None,
    }
}
