//! Compresses real uiautomator dumps and pins the rendered output with snapshots.

use mdh_driver::android::parse_hierarchy;
use mdh_ui::{RefTable, UiTree, compress, render};

fn load(name: &str) -> (String, UiTree) {
    let path = format!(
        "{}/../../fixtures/android/uiautomator/{name}.xml",
        env!("CARGO_MANIFEST_DIR")
    );
    let xml = std::fs::read_to_string(path).unwrap();
    let mut tree = compress(&parse_hierarchy(&xml).unwrap());
    RefTable::default().assign(&mut tree);
    (xml, tree)
}

/// Rough token estimate (≈4 bytes per token), good enough to catch compression regressions.
fn tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

fn snapshot(name: &str) -> String {
    let (xml, tree) = load(name);
    let text = render(&tree);
    format!(
        "raw: {} nodes, ~{} tokens | compact: {} nodes, ~{} tokens\n\n{text}",
        tree.raw_nodes,
        tokens(&xml),
        tree.iter().count(),
        tokens(&text),
    )
}

macro_rules! fixture_test {
    ($($name:ident),*) => {$(
        #[test]
        fn $name() {
            insta::assert_snapshot!(snapshot(concat!(stringify!($name), "_api36")));
        }
    )*};
}

fixture_test!(
    launcher,
    settings,
    settings_display,
    settings_network,
    settings_search
);
