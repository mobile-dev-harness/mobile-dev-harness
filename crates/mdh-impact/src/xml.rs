//! Android XML: layouts and menus, values, navigation graphs, the manifest, other resource files.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::model::FileKind;
use crate::model::{Decl, DeclKind, FileIndex, ManifestComponent, NavEdge, Receiver, Ref, RefKind};
use crate::source::{Classified, RESOURCE_TYPES};
use crate::syntax::{self, child_of_kind, line, named_children, text};

/// Longest string value kept for display.
const VALUE_CHARS: usize = 60;

pub fn extract(path: &str, src: &str, class: &Classified) -> FileIndex {
    let mut out = FileIndex {
        path: path.to_owned(),
        ..FileIndex::default()
    };
    let language = tree_sitter_xml::LANGUAGE_XML.into();
    let Some((tree, errors)) = syntax::parse(&language, src, false) else {
        out.errors = 1;
        return out;
    };
    out.errors = errors;
    let Some(root) = child_of_kind(tree.root_node(), "element") else {
        return out;
    };
    let x = Xml { src };
    match class.kind {
        FileKind::Values => x.values(root, &mut out),
        FileKind::Manifest => x.manifest(root, &mut out),
        FileKind::Navigation => {
            x.file_decl(path, class, root, &mut out);
            x.navigation(root, &mut out);
        }
        _ => {
            x.file_decl(path, class, root, &mut out);
            x.layout(root, &mut out);
        }
    }
    out
}

/// A resource file that isn't XML (an image, a font): one declaration whose body is the content hash.
pub fn binary(path: &str, class: &Classified, content_hash: u64) -> FileIndex {
    let mut out = FileIndex {
        path: path.to_owned(),
        ..FileIndex::default()
    };
    let mut d = Decl::new(DeclKind::Resource, file_stem(path), "", 1);
    d.rtype = class.res_dir.clone();
    d.key = format!("@{}/{}", class.res_dir.as_deref().unwrap_or("raw"), d.name);
    d.body = content_hash;
    out.decls.push(d);
    out
}

/// `res/drawable/ic_back.9.png` → `ic_back`.
pub fn file_stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.split('.').next().unwrap_or(file).to_owned()
}

struct Xml<'s> {
    src: &'s str,
}

struct Element<'t, 's> {
    node: Node<'t>,
    /// `STag` or `EmptyElemTag`.
    tag: Node<'t>,
    name: &'s str,
    attrs: Vec<(&'s str, &'s str)>,
}

impl Element<'_, '_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }
}

impl<'s> Xml<'s> {
    fn element<'t>(&self, node: Node<'t>) -> Option<Element<'t, 's>> {
        let tag = named_children(node)
            .into_iter()
            .find(|c| matches!(c.kind(), "STag" | "EmptyElemTag"))?;
        let name = child_of_kind(tag, "Name").map(|n| text(n, self.src))?;
        let attrs = named_children(tag)
            .into_iter()
            .filter(|c| c.kind() == "Attribute")
            .filter_map(|a| {
                let k = child_of_kind(a, "Name")?;
                let v = child_of_kind(a, "AttValue")?;
                let v = text(v, self.src);
                Some((
                    text(k, self.src),
                    v.get(1..v.len().saturating_sub(1)).unwrap_or(""),
                ))
            })
            .collect();
        Some(Element {
            node,
            tag,
            name,
            attrs,
        })
    }

    fn children<'t>(&self, node: Node<'t>) -> Vec<Element<'t, 's>> {
        child_of_kind(node, "content")
            .map(|c| {
                named_children(c)
                    .into_iter()
                    .filter(|n| n.kind() == "element")
                    .filter_map(|n| self.element(n))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Text content with markup and whitespace runs collapsed.
    fn text_content(&self, node: Node) -> String {
        let mut s = String::new();
        if let Some(c) = child_of_kind(node, "content") {
            let mut stack = vec![c];
            while let Some(n) = stack.pop() {
                match n.kind() {
                    "CharData" | "EntityRef" | "CharRef" | "CDSect" => {
                        s.push_str(text(n, self.src));
                        s.push(' ');
                    }
                    "Comment" => {}
                    _ => stack.extend(named_children(n).into_iter().rev()),
                }
            }
        }
        syntax::collapsed(&s)
    }

    /// Tokens without comments and whitespace-only text: formatting changes hash the same.
    fn hash(&self, node: Node) -> u64 {
        syntax::token_hash(node, self.src, &|n| {
            n.kind() == "Comment" || (n.kind() == "CharData" && text(n, self.src).trim().is_empty())
        })
    }

    fn file_decl(&self, path: &str, class: &Classified, root: Node, out: &mut FileIndex) {
        let rtype = class.res_dir.clone().unwrap_or_else(|| "xml".into());
        let mut d = Decl::new(DeclKind::Resource, file_stem(path), "", line(root));
        d.key = format!("@{rtype}/{}", d.name);
        d.rtype = Some(rtype);
        d.body = self.hash(root);
        out.decls.push(d);
    }

    /// `@string/title` and the like in attribute values and text, from declaration `from`.
    fn resource_refs(&self, value: &str, node: Node, from: Option<usize>, out: &mut FileIndex) {
        for (i, _) in value.match_indices('@') {
            let rest = &value[i + 1..];
            if rest.starts_with("android:") || rest.starts_with('+') {
                continue;
            }
            let rest = rest.strip_prefix('*').unwrap_or(rest);
            let Some((rtype, name)) = rest.split_once('/') else {
                continue;
            };
            let name: String = name
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if RESOURCE_TYPES.contains(&rtype) && !name.is_empty() {
                out.refs.push(Ref {
                    kind: RefKind::Resource(rtype.to_owned()),
                    name: name.replace('.', "_"),
                    receiver: Receiver::Implicit,
                    line: line(node),
                    from,
                    args: None,
                    trigger: None,
                });
            }
        }
    }

    fn type_ref(&self, class: &str, node: Node, from: Option<usize>, out: &mut FileIndex) {
        let name = class.rsplit(['.', '$']).next().unwrap_or(class);
        if !name.is_empty() {
            out.refs.push(Ref {
                kind: RefKind::Type,
                name: name.to_owned(),
                receiver: Receiver::Implicit,
                line: line(node),
                from,
                args: None,
                trigger: None,
            });
        }
    }

    fn layout(&self, root: Node, out: &mut FileIndex) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let Some(e) = self.element(node) else {
                continue;
            };
            if e.name.contains('.') {
                self.type_ref(e.name, e.tag, Some(0), out);
            }
            for (k, v) in &e.attrs {
                if (*k == "class"
                    || (*k == "android:name"
                        && matches!(
                            e.name,
                            "fragment" | "androidx.fragment.app.FragmentContainerView"
                        )))
                    && v.contains('.')
                {
                    self.type_ref(v, e.tag, Some(0), out);
                }
                self.resource_refs(v, e.tag, Some(0), out);
            }
            if let Some(id) = e.attr("android:id").and_then(|v| v.strip_prefix("@+id/")) {
                let mut d = Decl::new(DeclKind::Resource, id, "", line(e.tag));
                d.rtype = Some("id".into());
                d.key = format!("@id/{id}");
                d.body = self.hash(e.tag);
                out.decls.push(d);
                let label = [
                    "android:text",
                    "android:title",
                    "android:contentDescription",
                    "android:hint",
                ]
                .iter()
                .find_map(|a| e.attr(a));
                if let Some(label) = label {
                    out.labels.push((id.to_owned(), label.to_owned()));
                }
            }
            stack.extend(self.children(e.node).into_iter().rev().map(|c| c.node));
        }
    }

    fn values(&self, root: Node, out: &mut FileIndex) {
        for e in self.children(root) {
            let Some(name) = e.attr("name") else {
                continue;
            };
            let rtype = match e.name {
                "string-array" | "integer-array" | "array" => "array",
                "declare-styleable" => "styleable",
                "item" => e.attr("type").unwrap_or("item"),
                "eat-comment" | "skip" => continue,
                other => other,
            };
            let name = name.replace('.', "_");
            let mut d = Decl::new(DeclKind::Resource, &name, "", line(e.node));
            d.key = format!("@{rtype}/{name}");
            d.rtype = Some(rtype.to_owned());
            d.body = self.hash(e.node);
            if matches!(rtype, "string" | "color" | "dimen" | "integer" | "bool") {
                let value: String = self.text_content(e.node);
                d.value = Some(match value.char_indices().nth(VALUE_CHARS) {
                    Some((i, _)) => format!("{}…", &value[..i]),
                    None => value,
                });
            }
            out.decls.push(d);
            let from = Some(out.decls.len() - 1);
            if let Some(parent) = e.attr("parent") {
                let parent = parent.strip_prefix("@style/").unwrap_or(parent);
                if !parent.is_empty()
                    && !parent.starts_with("@android")
                    && !parent.starts_with("android:")
                {
                    self.resource_refs(&format!("@style/{parent}"), e.tag, from, out);
                }
            }
            let mut stack = vec![e.node];
            while let Some(n) = stack.pop() {
                match n.kind() {
                    "CharData" | "AttValue" => self.resource_refs(text(n, self.src), n, from, out),
                    _ => stack.extend(named_children(n)),
                }
            }
        }
    }

    fn navigation(&self, root: Node, out: &mut FileIndex) {
        let mut classes: HashMap<String, String> = HashMap::new();
        let mut actions: Vec<(String, String, Option<String>)> = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let Some(e) = self.element(node) else {
                continue;
            };
            let class = e.attr("android:name").filter(|_| e.name != "argument");
            if let Some(class) = class {
                self.type_ref(class, e.tag, Some(0), out);
            }
            if let (Some(id), Some(class)) = (e.attr("android:id"), class) {
                let id = id.trim_start_matches("@+id/").trim_start_matches("@id/");
                let simple = class.rsplit('.').next().unwrap_or(class);
                classes.insert(id.to_owned(), simple.to_owned());
                for a in self.children(e.node).iter().filter(|c| c.name == "action") {
                    if let Some(dest) = a.attr("app:destination") {
                        let action_id = a
                            .attr("android:id")
                            .map(|i| i.trim_start_matches("@+id/").to_owned());
                        actions.push((
                            simple.to_owned(),
                            dest.trim_start_matches("@id/").to_owned(),
                            action_id,
                        ));
                    }
                }
            }
            for (_, v) in &e.attrs {
                self.resource_refs(v, e.tag, Some(0), out);
            }
            stack.extend(self.children(e.node).into_iter().map(|c| c.node));
        }
        for (from, dest, trigger) in actions {
            if let Some(to) = classes.get(&dest) {
                out.edges.push(NavEdge {
                    from,
                    to: to.clone(),
                    trigger,
                });
            }
        }
    }

    fn manifest(&self, root: Node, out: &mut FileIndex) {
        let Some(m) = self.element(root) else {
            return;
        };
        if let Some(p) = m.attr("package") {
            out.package = p.to_owned();
        }
        for e in self.children(root) {
            match e.name {
                "application" => {
                    let mut d = Decl::new(DeclKind::Manifest, "application", "", line(e.node));
                    d.rtype = Some("application".into());
                    d.key = "<application>".into();
                    d.body = self.hash(e.tag);
                    out.decls.push(d);
                    let from = Some(out.decls.len() - 1);
                    for (k, v) in &e.attrs {
                        if *k == "android:name" {
                            self.type_ref(v, e.tag, from, out);
                        }
                        self.resource_refs(v, e.tag, from, out);
                    }
                    for c in self.children(e.node) {
                        self.component(&c, out);
                    }
                }
                _ => self.entry(&e, out),
            }
        }
    }

    /// A top-level manifest entry: `uses-permission`, `uses-feature`, `queries`, …
    fn entry(&self, e: &Element, out: &mut FileIndex) {
        let name = e
            .attr("android:name")
            .map(|n| n.rsplit('.').next().unwrap_or(n).to_owned())
            .unwrap_or_else(|| e.name.to_owned());
        let mut d = Decl::new(DeclKind::Manifest, &name, "", line(e.node));
        d.rtype = Some(e.name.to_owned());
        d.key = format!("<{}> {name}", e.name);
        d.body = self.hash(e.node);
        out.decls.push(d);
    }

    fn component(&self, e: &Element, out: &mut FileIndex) {
        let Some(class) = e.attr("android:name") else {
            return self.entry(e, out);
        };
        let simple = class.rsplit('.').next().unwrap_or(class).to_owned();
        let mut d = Decl::new(DeclKind::Manifest, &simple, "", line(e.node));
        d.rtype = Some(e.name.to_owned());
        d.key = format!("<{}> {simple}", e.name);
        d.body = self.hash(e.node);
        out.decls.push(d);
        let from = Some(out.decls.len() - 1);
        let target = e.attr("android:targetActivity").unwrap_or(class);
        self.type_ref(target, e.tag, from, out);
        let mut component = ManifestComponent {
            class: target.rsplit('.').next().unwrap_or(target).to_owned(),
            element: e.name.to_owned(),
            ..ManifestComponent::default()
        };
        for (_, v) in &e.attrs {
            self.resource_refs(v, e.tag, from, out);
        }
        for filter in self
            .children(e.node)
            .iter()
            .filter(|c| c.name == "intent-filter")
        {
            let parts = self.children(filter.node);
            let values = |tag: &str| -> Vec<&str> {
                parts
                    .iter()
                    .filter(|p| p.name == tag)
                    .filter_map(|p| p.attr("android:name"))
                    .collect()
            };
            let actions = values("action");
            let categories = values("category");
            if actions.contains(&"android.intent.action.MAIN")
                && categories.contains(&"android.intent.category.LAUNCHER")
            {
                component.launcher = true;
            }
            if actions.contains(&"android.intent.action.VIEW") {
                let data: Vec<&Element> = parts.iter().filter(|p| p.name == "data").collect();
                let all = |attr: &str| -> Vec<&str> {
                    data.iter().filter_map(|d| d.attr(attr)).collect()
                };
                let hosts = all("android:host");
                let paths: Vec<&str> =
                    ["android:path", "android:pathPrefix", "android:pathPattern"]
                        .iter()
                        .flat_map(|a| all(a))
                        .collect();
                for scheme in all("android:scheme") {
                    let hosts = if hosts.is_empty() {
                        vec![""]
                    } else {
                        hosts.clone()
                    };
                    for host in hosts {
                        let path = paths.first().copied().unwrap_or("");
                        component
                            .deep_links
                            .push(format!("{scheme}://{host}{path}"));
                    }
                }
            }
        }
        out.components.push(component);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::classify;

    fn index(path: &str, src: &str) -> FileIndex {
        extract(path, src, &classify(path))
    }

    #[test]
    fn layout_ids_labels_and_references() {
        let f = index(
            "app/src/main/res/layout/activity_main.xml",
            r#"<LinearLayout xmlns:android="x">
  <!-- buttons -->
  <Button android:id="@+id/open_login" android:text="Log in" android:background="@drawable/bg" />
  <dev.mdh.Chart android:id="@+id/chart" android:contentDescription="@string/chart" />
</LinearLayout>"#,
        );
        let keys: Vec<&str> = f.decls.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(
            keys,
            ["@layout/activity_main", "@id/open_login", "@id/chart"]
        );
        assert_eq!(
            f.labels,
            [
                ("open_login".into(), "Log in".into()),
                ("chart".into(), "@string/chart".into())
            ]
        );
        let refs: Vec<String> = f
            .refs
            .iter()
            .map(|r| format!("{:?} {}", r.kind, r.name))
            .collect();
        assert!(
            refs.contains(&"Resource(\"drawable\") bg".to_string()),
            "{refs:?}"
        );
        assert!(
            refs.contains(&"Resource(\"string\") chart".to_string()),
            "{refs:?}"
        );
        assert!(refs.contains(&"Type Chart".to_string()), "{refs:?}");
    }

    #[test]
    fn formatting_and_comments_keep_the_hash() {
        let a = index("res/layout/a.xml", "<A>\n  <B x=\"1\"/>\n</A>");
        let b = index(
            "res/layout/a.xml",
            "<A><!-- note -->\n\n      <B x=\"1\"/></A>",
        );
        let c = index("res/layout/a.xml", "<A><B x=\"2\"/></A>");
        assert_eq!(a.decls[0].body, b.decls[0].body);
        assert_ne!(a.decls[0].body, c.decls[0].body);
    }

    #[test]
    fn values_entries() {
        let f = index(
            "app/src/main/res/values/strings.xml",
            r#"<resources>
  <string name="sign_in">Sign <b>in</b></string>
  <style name="Theme.App" parent="Theme.Material3.Light"><item name="colorPrimary">@color/brand</item></style>
</resources>"#,
        );
        let keys: Vec<&str> = f.decls.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, ["@string/sign_in", "@style/Theme_App"]);
        assert_eq!(f.decls[0].value.as_deref(), Some("Sign in"));
        let refs: Vec<String> = f
            .refs
            .iter()
            .map(|r| format!("{:?} {}", r.kind, r.name))
            .collect();
        assert!(
            refs.contains(&"Resource(\"color\") brand".to_string()),
            "{refs:?}"
        );
        assert!(
            refs.contains(&"Resource(\"style\") Theme_Material3_Light".to_string()),
            "{refs:?}"
        );
    }

    #[test]
    fn manifest_components_launcher_and_deep_links() {
        let f = index(
            "app/src/main/AndroidManifest.xml",
            r#"<manifest xmlns:android="x">
  <uses-permission android:name="android.permission.CAMERA" />
  <application android:label="x">
    <activity android:name=".MainActivity" android:exported="true">
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
    <activity android:name="dev.x.LoginActivity">
      <intent-filter>
        <action android:name="android.intent.action.VIEW" />
        <data android:scheme="mdhsample" android:host="login" />
      </intent-filter>
    </activity>
  </application>
</manifest>"#,
        );
        let keys: Vec<&str> = f.decls.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "<uses-permission> CAMERA",
                "<application>",
                "<activity> MainActivity",
                "<activity> LoginActivity"
            ]
        );
        assert!(f.components[0].launcher);
        assert_eq!(f.components[1].deep_links, ["mdhsample://login"]);
    }

    #[test]
    fn navigation_graph_edges() {
        let f = index(
            "app/src/main/res/navigation/nav.xml",
            r#"<navigation xmlns:android="x" xmlns:app="y">
  <fragment android:id="@+id/home" android:name="a.HomeFragment">
    <action android:id="@+id/to_detail" app:destination="@id/detail" />
  </fragment>
  <fragment android:id="@+id/detail" android:name="a.DetailFragment" />
</navigation>"#,
        );
        assert_eq!(
            f.edges,
            [NavEdge {
                from: "HomeFragment".into(),
                to: "DetailFragment".into(),
                trigger: Some("to_detail".into())
            }]
        );
    }
}
