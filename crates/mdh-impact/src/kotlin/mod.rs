//! Declarations and references in Kotlin sources (`tree-sitter-kotlin-ng`).

use std::collections::HashMap;

use tree_sitter::Node;

use crate::model::{Decl, DeclKind, FileIndex, Receiver, Ref, RefKind};
use crate::source::{RESOURCE_TYPES, binding_layout, literal_file_name, snake_case};
use crate::syntax::{self, child_of_kind, children, line, named_children, text};

/// Annotations whose argument is the API level a declaration needs.
pub(crate) const API_ANNOTATIONS: &[&str] = &["RequiresApi", "TargetApi", "ChecksSdkIntAtLeast"];

/// Members of a type body; a type's own body hash leaves them out because they are diffed on their own.
const MEMBERS: &[&str] = &[
    "function_declaration",
    "property_declaration",
    "class_declaration",
    "object_declaration",
    "companion_object",
    "secondary_constructor",
    "type_alias",
    "enum_entry",
];

pub fn extract(path: &str, src: &str) -> FileIndex {
    let language = tree_sitter_kotlin_ng::LANGUAGE.into();
    let mut out = FileIndex {
        path: path.to_owned(),
        ..FileIndex::default()
    };
    let Some((tree, errors)) = syntax::parse(&language, src, true) else {
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
    /// Enclosing declarations, innermost last.
    stack: Vec<usize>,
    /// Names of the enclosing types.
    owners: Vec<String>,
    /// Declared or constructed types of properties and parameters, by name (scope-insensitive).
    var_types: HashMap<String, String>,
    /// Views by variable: `val signIn = findViewById<Button>(R.id.sign_in)`.
    var_ids: HashMap<String, String>,
}

impl<'s> Extractor<'s> {
    fn text(&self, node: Node) -> &'s str {
        text(node, self.src)
    }

    fn from(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    /// Inside a function or property body, where declarations are local.
    fn local(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|&i| !self.out.decls[i].kind.is_type())
    }

    fn push_ref(&mut self, kind: RefKind, name: &str, receiver: Receiver, node: Node) {
        if name.is_empty() {
            return;
        }
        if matches!(kind, RefKind::Call | RefKind::Type | RefKind::Name)
            && let Some(layout) = binding_layout(name)
        {
            self.push(Ref {
                kind: RefKind::Resource("layout".into()),
                name: layout,
                receiver: Receiver::Implicit,
                line: line(node),
                from: self.from(),
                args: None,
                trigger: None,
            });
        }
        self.push(Ref {
            kind,
            name: name.to_owned(),
            receiver,
            line: line(node),
            from: self.from(),
            args: None,
            trigger: None,
        });
    }

    fn push(&mut self, r: Ref) {
        if self.out.refs.last() != Some(&r) {
            self.out.refs.push(r);
        }
    }

    fn visit(&mut self, node: Node) {
        match node.kind() {
            "package_header" => {
                if let Some(q) = named_children(node).first() {
                    self.out.package = self.text(*q).to_owned();
                }
            }
            "import" => {
                if let Some(q) = child_of_kind(node, "qualified_identifier") {
                    let mut name = self.text(q).to_owned();
                    if self.text(node).trim_end().ends_with('*') {
                        name.push_str(".*");
                    }
                    self.out.imports.push(name);
                }
            }
            "class_declaration" | "object_declaration" | "companion_object" => self.type_decl(node),
            "type_alias" => self.type_alias(node),
            "function_declaration" => self.function(node),
            "secondary_constructor" => self.constructor(node),
            "property_declaration" => self.property(node),
            "enum_entry" => self.enum_entry(node),
            "call_expression" => self.call(node),
            "navigation_expression" => self.navigation(node),
            "callable_reference" => {
                for c in named_children(node) {
                    match c.kind() {
                        "identifier" => {
                            let name = self.text(c);
                            self.push_ref(RefKind::Name, name, Receiver::Implicit, c);
                        }
                        _ => self.visit(c),
                    }
                }
            }
            "user_type" => {
                let ids: Vec<Node> = named_children(node)
                    .into_iter()
                    .filter(|c| c.kind() == "identifier")
                    .collect();
                if let Some(last) = ids.last() {
                    let name = self.text(*last);
                    self.push_ref(RefKind::Type, name, Receiver::Implicit, *last);
                }
                for c in named_children(node) {
                    if c.kind() != "identifier" {
                        self.visit(c);
                    }
                }
            }
            "identifier" => {
                let name = self.text(node);
                self.push_ref(RefKind::Name, name, Receiver::Implicit, node);
            }
            // Names being declared, not used.
            "parameter" | "class_parameter" | "variable_declaration" | "type_parameter" => {
                self.visit_skipping_first_identifier(node);
            }
            "value_argument" => {
                let named = children(node).iter().any(|c| c.kind() == "=");
                if named {
                    self.visit_skipping_first_identifier(node);
                } else {
                    self.visit_children(node);
                }
            }
            "string_content" => {
                if let Some(file) = literal_file_name(self.text(node)) {
                    self.push_ref(RefKind::Literal, &file, Receiver::Implicit, node);
                }
            }
            "label" | "this_expression" | "super_expression" | "line_comment" | "block_comment" => {
            }
            "binary_expression" => {
                self.api_check(node);
                self.visit_children(node);
            }
            _ => self.visit_children(node),
        }
    }

    /// Records the API level an `SDK_INT` comparison branches on, for the enclosing declaration.
    fn api_check(&mut self, node: Node) {
        let expr = self.text(node);
        if expr.contains("SDK_INT")
            && let Some(level) = syntax::api_level(expr)
            && let Some(i) = self.from()
            && !self.out.decls[i].api_levels.contains(&level)
        {
            self.out.decls[i].api_levels.push(level);
        }
    }

    fn visit_children(&mut self, node: Node) {
        for c in named_children(node) {
            self.visit(c);
        }
    }

    fn visit_skipping_first_identifier(&mut self, node: Node) {
        let mut skipped = false;
        for c in named_children(node) {
            if !skipped && c.kind() == "identifier" {
                skipped = true;
                continue;
            }
            self.visit(c);
        }
    }

    fn open(&mut self, decl: Decl) -> usize {
        self.out.decls.push(decl);
        let i = self.out.decls.len() - 1;
        self.stack.push(i);
        i
    }

    fn owner(&self) -> String {
        self.owners.join(".")
    }

    fn modifiers(&self, node: Node, decl: &mut Decl) {
        let Some(m) = child_of_kind(node, "modifiers") else {
            return;
        };
        for c in named_children(m) {
            match c.kind() {
                "annotation" => {
                    if let Some(t) = first_descendant(c, "user_type") {
                        let ids = named_children(t);
                        if let Some(id) = ids.iter().rev().find(|i| i.kind() == "identifier") {
                            let name = self.text(*id);
                            if API_ANNOTATIONS.contains(&name)
                                && let Some(level) = syntax::api_level(self.text(c))
                            {
                                decl.api_levels.push(level);
                            }
                            decl.annotations.push(name.to_owned());
                        }
                    }
                }
                "member_modifier" if self.text(c) == "override" => decl.overrides = true,
                _ => {}
            }
        }
    }

    fn type_decl(&mut self, node: Node) {
        let name = child_of_kind(node, "identifier")
            .map(|n| self.text(n).to_owned())
            .unwrap_or_else(|| "Companion".into());
        let kind = match node.kind() {
            "class_declaration" if children(node).iter().any(|c| c.kind() == "interface") => {
                DeclKind::Interface
            }
            "class_declaration"
                if child_of_kind(node, "modifiers")
                    .is_some_and(|m| self.text(m).split_whitespace().any(|w| w == "enum")) =>
            {
                DeclKind::Enum
            }
            "class_declaration" => DeclKind::Class,
            _ => DeclKind::Object,
        };
        let mut decl = Decl::new(kind, &name, self.owner(), line(node));
        self.modifiers(node, &mut decl);
        if let Some(specs) = child_of_kind(node, "delegation_specifiers") {
            for spec in named_children(specs) {
                if let Some(t) = first_descendant(spec, "user_type")
                    && let Some(id) = named_children(t)
                        .iter()
                        .rev()
                        .find(|i| i.kind() == "identifier")
                {
                    decl.supertypes.push(self.text(*id).to_owned());
                }
            }
        }
        if let Some(ctor) = child_of_kind(node, "primary_constructor") {
            decl.params = Some(syntax::collapsed(self.text(ctor)));
            decl.arity = child_of_kind(ctor, "class_parameters").map(|p| arity(p, self.src));
        }
        let is_body = |n: Node| matches!(n.kind(), "class_body" | "enum_class_body");
        decl.signature = norm(node, self.src, &is_body);
        decl.body = child_of_kind(node, "class_body")
            .or_else(|| child_of_kind(node, "enum_class_body"))
            .map_or(0, |b| {
                syntax::token_hash(b, self.src, &|n| MEMBERS.contains(&n.kind()))
            });
        if self.local() {
            // A local or anonymous class: its uses count as uses by the enclosing function.
            self.visit_skipping_first_identifier(node);
            return;
        }
        self.open(decl);
        self.owners.push(name);
        self.visit_skipping_first_identifier(node);
        self.owners.pop();
        self.stack.pop();
    }

    fn type_alias(&mut self, node: Node) {
        let Some(id) = child_of_kind(node, "identifier") else {
            return self.visit_children(node);
        };
        let mut decl = Decl::new(DeclKind::TypeAlias, self.text(id), self.owner(), line(node));
        decl.signature = norm(node, self.src, &|_| false);
        self.open(decl);
        self.visit_skipping_first_identifier(node);
        self.stack.pop();
    }

    fn function(&mut self, node: Node) {
        let kids = named_children(node);
        let Some(name_pos) = kids.iter().position(|c| c.kind() == "identifier") else {
            return self.visit_children(node);
        };
        if self.local() {
            return self.visit_except(node, kids[name_pos]);
        }
        let mut decl = Decl::new(
            DeclKind::Function,
            self.text(kids[name_pos]),
            self.owner(),
            line(kids[name_pos]),
        );
        self.modifiers(node, &mut decl);
        decl.extends = kids[..name_pos]
            .iter()
            .rev()
            .find(|c| matches!(c.kind(), "user_type" | "nullable_type"))
            .and_then(|t| last_type_name(*t, self.src));
        if let Some(params) = child_of_kind(node, "function_value_parameters") {
            decl.params = Some(syntax::collapsed(self.text(params)));
            decl.arity = Some(arity(params, self.src));
        }
        decl.signature = norm(node, self.src, &|n| n.kind() == "function_body");
        decl.body = child_of_kind(node, "function_body")
            .map_or(0, |b| syntax::token_hash(b, self.src, &|_| false));
        self.open(decl);
        self.visit_except(node, kids[name_pos]);
        self.stack.pop();
    }

    fn constructor(&mut self, node: Node) {
        let Some(class) = self.owners.last().cloned() else {
            return self.visit_children(node);
        };
        let mut decl = Decl::new(DeclKind::Constructor, &class, self.owner(), line(node));
        decl.key = format!("{}.<init>", self.owner());
        self.modifiers(node, &mut decl);
        if let Some(params) = child_of_kind(node, "function_value_parameters") {
            decl.params = Some(syntax::collapsed(self.text(params)));
            decl.arity = Some(arity(params, self.src));
        }
        decl.signature = norm(node, self.src, &|n| n.kind() == "block");
        decl.body = syntax::token_hash(node, self.src, &|n| {
            matches!(n.kind(), "modifiers" | "function_value_parameters")
        });
        self.open(decl);
        self.visit_children(node);
        self.stack.pop();
    }

    fn property(&mut self, node: Node) {
        let kids = children(node);
        let Some(decl_pos) = kids.iter().position(|c| {
            matches!(
                c.kind(),
                "variable_declaration" | "multi_variable_declaration"
            )
        }) else {
            return self.visit_children(node);
        };
        if self.local() {
            return self.visit_children(node);
        }
        let Some(id) = first_descendant(kids[decl_pos], "identifier") else {
            return self.visit_children(node);
        };
        let mut decl = Decl::new(DeclKind::Property, self.text(id), self.owner(), line(id));
        self.modifiers(node, &mut decl);
        let mut signature = String::new();
        let mut body = String::new();
        for (i, c) in kids.iter().enumerate() {
            let out = if i <= decl_pos {
                &mut signature
            } else {
                &mut body
            };
            syntax::tokens(*c, self.src, &|_| false, out);
        }
        decl.signature = signature;
        decl.body = syntax::hash(&body);
        self.open(decl);
        self.visit_children(node);
        self.stack.pop();
    }

    fn enum_entry(&mut self, node: Node) {
        let Some(id) = child_of_kind(node, "identifier") else {
            return self.visit_children(node);
        };
        let mut decl = Decl::new(DeclKind::Property, self.text(id), self.owner(), line(node));
        decl.signature = self.text(id).to_owned();
        decl.body = syntax::token_hash(node, self.src, &|_| false);
        self.open(decl);
        self.visit_skipping_first_identifier(node);
        self.stack.pop();
    }

    fn visit_except(&mut self, node: Node, skip: Node) {
        for c in named_children(node) {
            if c.id() != skip.id() {
                self.visit(c);
            }
        }
    }

    fn call(&mut self, node: Node) {
        let kids = named_children(node);
        let Some(&callee) = kids.first() else {
            return;
        };
        let args = kids
            .iter()
            .map(|k| match k.kind() {
                "value_arguments" => named_children(*k)
                    .iter()
                    .filter(|a| a.kind() == "value_argument")
                    .count(),
                "annotated_lambda" => 1,
                _ => 0,
            })
            .sum();
        match callee.kind() {
            "identifier" => {
                let name = self.text(callee);
                self.push_ref(RefKind::Call, name, Receiver::Implicit, callee);
                self.set_args(args);
            }
            "navigation_expression" if !self.is_resource_or_literal(callee) => {
                let parts = named_children(callee);
                if let (Some(&left), Some(&right)) = (parts.first(), parts.last())
                    && right.kind() == "identifier"
                    && parts.len() == 2
                {
                    let receiver = self.receiver_of(left);
                    let name = self.text(right);
                    self.push_ref(RefKind::Call, name, receiver, right);
                    self.set_args(args);
                    self.visit(left);
                } else {
                    self.visit(callee);
                }
            }
            _ => self.visit(callee),
        }
        for k in &kids[1..] {
            self.visit(*k);
        }
    }

    fn set_args(&mut self, args: usize) {
        if let Some(r) = self.out.refs.last_mut()
            && r.kind == RefKind::Call
        {
            r.args = Some(args);
        }
    }

    fn is_resource_or_literal(&self, node: Node) -> bool {
        self.resource(node).is_some() || self.class_literal(node).is_some()
    }

    /// `R.layout.main`, `com.example.R.string.title` → (`layout`, `main`). Framework resources
    /// (`android.R.…`) are not the app's.
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

    /// `Foo::class` → `Foo`.
    fn class_literal(&self, node: Node) -> Option<String> {
        let parts = children(node);
        let [left, op, right] = parts.as_slice() else {
            return None;
        };
        (op.kind() == "::" && self.text(*right) == "class")
            .then(|| last_segment(*left, self.src))
            .flatten()
    }

    fn navigation(&mut self, node: Node) {
        if let Some((rtype, name)) = self.resource(node) {
            if !rtype.is_empty() {
                self.push_ref(RefKind::Resource(rtype), &name, Receiver::Implicit, node);
            }
            return;
        }
        if let Some(class) = self.class_literal(node) {
            self.push_ref(RefKind::ClassLiteral, &class, Receiver::Implicit, node);
            let trigger = self.trigger(node);
            if let Some(r) = self.out.refs.last_mut() {
                r.trigger = trigger;
            }
            return;
        }
        let parts = named_children(node);
        let (Some(&left), Some(&right)) = (parts.first(), parts.last()) else {
            return;
        };
        if parts.len() != 2 || right.kind() != "identifier" {
            return self.visit_children(node);
        }
        let name = self.text(right);
        // `Foo::class.java`: `java` is the class literal's, not a property of the app's.
        if !(name == "java" && self.class_literal(left).is_some()) {
            let receiver = self.receiver_of(left);
            self.push_ref(RefKind::Name, name, receiver, right);
        }
        self.visit(left);
    }

    fn receiver_of(&self, node: Node) -> Receiver {
        match node.kind() {
            "identifier" => {
                let name = self.text(node);
                match self.var_types.get(name) {
                    Some(t) => Receiver::Type(t.clone()),
                    None if starts_upper(name) => Receiver::Type(name.to_owned()),
                    None => Receiver::Unknown,
                }
            }
            "this_expression" | "super_expression" => Receiver::Implicit,
            "call_expression" => match named_children(node).first() {
                Some(c) if c.kind() == "identifier" && starts_upper(self.text(*c)) => {
                    Receiver::Type(self.text(*c).to_owned())
                }
                _ => Receiver::Unknown,
            },
            "navigation_expression" => match chain(node, self.src) {
                Some(segments) if segments.last().is_some_and(|s| starts_upper(s)) => {
                    Receiver::Type(segments.last().unwrap().to_string())
                }
                _ => Receiver::Unknown,
            },
            _ => Receiver::Unknown,
        }
    }

    /// The view id that leads to a navigation: an `R.id` right next to the class literal
    /// (`R.id.open_login to LoginActivity::class.java`), or the view whose click listener contains it.
    fn trigger(&self, node: Node) -> Option<String> {
        let mut current = node;
        let mut level = 0;
        while let Some(parent) = current.parent() {
            level += 1;
            if matches!(
                parent.kind(),
                "function_declaration" | "class_body" | "source_file" | "property_declaration"
            ) {
                break;
            }
            if parent.kind() == "call_expression"
                && let Some(callee) = named_children(parent).first()
                && callee.kind() == "navigation_expression"
            {
                let parts = named_children(*callee);
                if let (Some(&left), Some(&right)) = (parts.first(), parts.last())
                    && is_click_listener(self.text(right))
                {
                    if let Some(id) = self.view_id_in(left, 20) {
                        return Some(id);
                    }
                    if left.kind() == "identifier"
                        && let Some(id) = self.var_ids.get(self.text(left))
                    {
                        return Some(id.clone());
                    }
                    // View binding: `binding.signIn.setOnClickListener`.
                    if left.kind() == "navigation_expression"
                        && let Some(last) =
                            chain(left, self.src).and_then(|s| s.last().map(|x| x.to_string()))
                    {
                        return Some(snake_case(&last));
                    }
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

    /// The first `R.id.x` in `node`, looking at no more than `budget` nodes.
    fn view_id_in(&self, node: Node, budget: usize) -> Option<String> {
        let mut stack = vec![node];
        let mut seen = 0;
        while let Some(n) = stack.pop() {
            seen += 1;
            if seen > budget {
                return None;
            }
            if n.kind() == "navigation_expression"
                && let Some((rtype, name)) = self.resource(n)
                && rtype == "id"
            {
                return Some(name);
            }
            stack.extend(named_children(n).into_iter().rev());
        }
        None
    }

    /// Types and view ids of properties and parameters, before extraction needs them.
    fn collect_vars(&mut self, node: Node) {
        match node.kind() {
            "property_declaration" => {
                let kids = named_children(node);
                if let Some(v) = kids.iter().find(|c| c.kind() == "variable_declaration")
                    && let Some(id) = child_of_kind(*v, "identifier")
                {
                    let name = self.text(id).to_owned();
                    let declared = named_children(*v)
                        .into_iter()
                        .find(|c| matches!(c.kind(), "user_type" | "nullable_type"))
                        .and_then(|t| last_type_name(t, self.src));
                    let init = kids
                        .iter()
                        .skip_while(|c| c.kind() != "variable_declaration")
                        .nth(1);
                    let inferred = init.and_then(|i| self.initializer_type(*i));
                    if let Some(t) = declared.or(inferred) {
                        self.var_types.insert(name.clone(), t);
                    }
                    if let Some(i) = init
                        && let Some(id) = self.view_id_in(*i, 30)
                    {
                        self.var_ids.insert(name, id);
                    }
                }
            }
            "parameter" | "class_parameter" => {
                let kids = named_children(node);
                if let Some(id) = kids.iter().find(|c| c.kind() == "identifier")
                    && let Some(t) = kids
                        .iter()
                        .find(|c| matches!(c.kind(), "user_type" | "nullable_type"))
                        .and_then(|t| last_type_name(*t, self.src))
                {
                    self.var_types.insert(self.text(*id).to_owned(), t);
                }
            }
            _ => {}
        }
        for c in named_children(node) {
            self.collect_vars(c);
        }
    }

    /// `Repo()` → `Repo`; `by viewModels<LoginViewModel>()` → `LoginViewModel`.
    fn initializer_type(&self, node: Node) -> Option<String> {
        let call = match node.kind() {
            "call_expression" => node,
            "property_delegate" => child_of_kind(node, "call_expression")?,
            _ => return None,
        };
        let kids = named_children(call);
        let callee = kids.first()?;
        if callee.kind() == "identifier" && starts_upper(self.text(*callee)) {
            return Some(self.text(*callee).to_owned());
        }
        let args = kids.iter().find(|k| k.kind() == "type_arguments")?;
        first_descendant(*args, "user_type").and_then(|t| last_type_name(t, self.src))
    }
}

fn is_click_listener(name: &str) -> bool {
    name.starts_with("setOn") && name.ends_with("Listener")
}

fn starts_upper(s: &str) -> bool {
    s.chars().next().is_some_and(char::is_uppercase)
}

fn norm(node: Node, src: &str, skip: &dyn Fn(Node) -> bool) -> String {
    let mut s = String::new();
    syntax::tokens(node, src, skip, &mut s);
    s
}

/// Required and maximum argument counts of a parameter list.
fn arity(params: Node, src: &str) -> (usize, Option<usize>) {
    let mut total = 0;
    let mut required = 0;
    let mut vararg = false;
    let mut pending: Option<bool> = None;
    for c in children(params) {
        match c.kind() {
            "parameter" | "class_parameter" => {
                if let Some(r) = pending.take() {
                    required += usize::from(r);
                }
                total += 1;
                let has_default = children(c).iter().any(|k| k.kind() == "=");
                pending = Some(!has_default);
                if text(c, src).contains("vararg") {
                    vararg = true;
                }
            }
            "parameter_modifiers" if text(c, src).contains("vararg") => vararg = true,
            "=" => pending = Some(false),
            _ => {}
        }
    }
    if let Some(r) = pending {
        required += usize::from(r);
    }
    (required, (!vararg).then_some(total))
}

/// Identifiers of a pure `a.b.C` chain.
fn chain<'s>(node: Node, src: &'s str) -> Option<Vec<&'s str>> {
    match node.kind() {
        "identifier" => Some(vec![text(node, src)]),
        "navigation_expression" => {
            let parts = children(node);
            let [left, op, right] = parts.as_slice() else {
                return None;
            };
            if op.kind() != "." || right.kind() != "identifier" {
                return None;
            }
            let mut segments = chain(*left, src)?;
            segments.push(text(*right, src));
            Some(segments)
        }
        _ => None,
    }
}

fn last_segment(node: Node, src: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(text(node, src).to_owned()),
        "user_type" => last_type_name(node, src),
        _ => chain(node, src).and_then(|s| s.last().map(|x| x.to_string())),
    }
}

fn last_type_name(node: Node, src: &str) -> Option<String> {
    let t = if node.kind() == "user_type" {
        node
    } else {
        first_descendant(node, "user_type")?
    };
    named_children(t)
        .iter()
        .rev()
        .find(|c| c.kind() == "identifier")
        .map(|c| text(*c, src).to_owned())
}

fn first_descendant<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == kind && n.id() != node.id() {
            return Some(n);
        }
        stack.extend(named_children(n).into_iter().rev());
    }
    None
}

#[cfg(test)]
mod tests;
