//! Compresses real captures (uiautomator dumps, the on-device helper's answer) and pins the
//! rendered output with snapshots.

use mdh_core::ui::{RawNode, ScreenInfo, WindowInfo};
use mdh_driver::android::parse_hierarchy;
use mdh_observe::{RefTable, UiTree, compress, render};

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
    nia_topic_under_status_bar,
    settings,
    settings_display,
    settings_network,
    settings_search
);

/// What the on-device helper answers to `tree`.
#[derive(serde::Deserialize)]
struct HelperTree {
    roots: Vec<RawNode>,
    windows: Vec<WindowInfo>,
}

/// A helper capture compressed as a session does it: the windows over the app decide what is
/// obscured.
fn load_helper(name: &str) -> UiTree {
    let path = format!(
        "{}/../../fixtures/android/helper/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let capture: HelperTree =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut tree = compress(&capture.roots);
    let screen = ScreenInfo::new(None, tree.screen, &capture.windows);
    tree.mark_obscured(&screen.obstructions);
    RefTable::default().assign(&mut tree);
    tree
}

/// Now in Android's topic screen with its top inset lost: the back button and the follow chip
/// are drawn under the status bar. The helper used to leave them out (Android reports what lies
/// under another window as not visible to the user); they are on screen, obscured.
#[test]
fn helper_keeps_what_the_app_draws_under_the_status_bar() {
    let text = render(&load_helper("nia_topic_under_status_bar_api36"));
    insta::assert_snapshot!(text);
    // `uiautomator dump` of the same screen: the same elements, with no windows to cover them.
    let (_, dumped) = load("nia_topic_under_status_bar_api36");
    assert_eq!(render(&dumped), text.replace(" obscured", ""));
}
