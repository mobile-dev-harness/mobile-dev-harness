//! Targeted compression, ref and diff behaviors on synthetic hierarchies.

use mdh_core::ui::{NodeFlags, RawNode, Rect};
use mdh_observe::{RefTable, UiTree, compress, diff, render, render_diff};

const SCREEN: Rect = Rect {
    left: 0,
    top: 0,
    right: 1000,
    bottom: 2000,
};

fn node(class: &str, bounds: Rect) -> RawNode {
    RawNode {
        class: format!("android.widget.{class}"),
        bounds,
        flags: NodeFlags {
            enabled: true,
            ..NodeFlags::default()
        },
        ..RawNode::default()
    }
}

fn text(s: &str, bounds: Rect) -> RawNode {
    RawNode {
        text: Some(s.into()),
        ..node("TextView", bounds)
    }
}

fn clickable(mut n: RawNode) -> RawNode {
    n.flags.clickable = true;
    n
}

fn screen(children: Vec<RawNode>) -> Vec<RawNode> {
    vec![RawNode {
        children,
        ..node("FrameLayout", SCREEN)
    }]
}

fn observe(refs: &mut RefTable, roots: &[RawNode]) -> UiTree {
    let mut tree = compress(roots);
    refs.assign(&mut tree);
    tree
}

fn row(title: &str, top: i32, checked: bool) -> RawNode {
    let mut switch = clickable(node("Switch", Rect::new(800, top, 950, top + 100)));
    switch.flags.checkable = true;
    switch.flags.checked = checked;
    RawNode {
        children: vec![text(title, Rect::new(50, top, 500, top + 100)), switch],
        ..clickable(node("LinearLayout", Rect::new(0, top, 1000, top + 100)))
    }
}

#[test]
fn row_with_single_switch_becomes_the_switch() {
    let tree = compress(&screen(vec![row("Dark theme", 100, false)]));
    assert_eq!(render(&tree), r#"[] switch "Dark theme" off"#);
}

/// Now in Android's onboarding chips: a selectable row around its own checkbox, both named by the
/// topic. One control, so the topic's name matches once.
#[test]
fn a_toggle_around_its_twin_is_one_control() {
    let chip = |name: &str, top: i32, checked: bool| {
        let mut inner = clickable(node("CheckBox", Rect::new(40, top + 20, 120, top + 80)));
        inner.flags.checkable = true;
        inner.flags.checked = checked;
        inner.text = Some(name.into());
        let mut outer = clickable(node("View", Rect::new(0, top, 600, top + 100)));
        outer.flags.checkable = true;
        outer.flags.checked = checked;
        outer.children = vec![text(name, Rect::new(140, top, 600, top + 100)), inner];
        outer
    };
    let tree = observe(
        &mut RefTable::default(),
        &screen(vec![chip("Headlines", 100, false), chip("UI", 300, true)]),
    );
    let out = render(&tree);
    assert!(out.contains(r#"checkbox "Headlines" unchecked"#), "{out}");
    assert_eq!(out.matches("Headlines").count(), 1, "{out}");
    assert_eq!(out.matches(r#""UI""#).count(), 1, "{out}");
}

#[test]
fn textbox_separates_hint_from_value_and_masks_passwords() {
    let mut email = node("EditText", Rect::new(0, 0, 1000, 100));
    email.hint = Some("Email".into());
    email.text = Some("Email".into()); // empty field reporting its hint as text
    let mut password = node("EditText", Rect::new(0, 100, 1000, 200));
    password.hint = Some("Password".into());
    password.text = Some("hunter2".into());
    password.flags.password = true;

    let rendered = render(&compress(&screen(vec![email, password])));
    assert_eq!(
        rendered,
        "[] textbox \"Email\" empty\n[] textbox \"Password\" value=••••"
    );
    assert!(!rendered.contains("hunter2"));
}

#[test]
fn screen_sized_clickable_container_is_layout() {
    let root = RawNode {
        children: vec![clickable(text("OK", Rect::new(0, 0, 200, 100)))],
        ..clickable(node("FrameLayout", SCREEN))
    };
    assert_eq!(render(&compress(&[root])), r#"[] button "OK""#);
}

#[test]
fn undescribed_canvas_is_reported_but_covered_background_is_not() {
    let canvas = RawNode {
        class: "com.example.SignatureView".into(),
        ..node("View", Rect::new(0, 0, 1000, 1000))
    };
    let tree = compress(&screen(vec![canvas]));
    assert!(render(&tree).contains("undescribed region at [0,0][1000,1000]"));

    let background = RawNode {
        class: "com.example.Backdrop".into(),
        ..node("View", SCREEN)
    };
    let tree = compress(&screen(vec![
        background,
        clickable(text("A", Rect::new(0, 0, 1000, 500))),
    ]));
    assert!(tree.opaque.is_empty());
}

#[test]
fn refs_stay_stable_and_diff_reports_changes() {
    let mut refs = RefTable::default();
    let before = observe(
        &mut refs,
        &screen(vec![row("Wi-Fi", 0, false), row("Bluetooth", 100, false)]),
    );
    assert_eq!(
        render(&before),
        "[e1] switch \"Wi-Fi\" off\n[e2] switch \"Bluetooth\" off"
    );

    let after = observe(
        &mut refs,
        &screen(vec![
            row("Wi-Fi", 0, true),
            clickable(text("Add network", Rect::new(0, 300, 1000, 400))),
        ]),
    );
    // Unchanged elements keep their refs; new ones continue the numbering.
    assert_eq!(after.find("e1").unwrap().label.as_deref(), Some("Wi-Fi"));
    assert_eq!(
        render_diff(&diff(&before, &after)),
        "+ [e3] button \"Add network\"\n~ [e1] switch \"Wi-Fi\": off → on\n- e2"
    );
}

#[test]
fn identical_siblings_get_distinct_refs() {
    let mut refs = RefTable::default();
    let tree = observe(
        &mut refs,
        &screen(vec![
            clickable(text("Delete", Rect::new(0, 0, 100, 100))),
            clickable(text("Delete", Rect::new(0, 100, 100, 200))),
        ]),
    );
    assert_eq!(
        render(&tree),
        "[e1] button \"Delete\"\n[e2] button \"Delete\""
    );
}

#[test]
fn empty_generic_views_are_spacers_not_unreadable() {
    let spacer = node("View", Rect::new(0, 0, 1000, 1000)); // android.widget.View, see `node`
    let spacer = RawNode {
        class: "android.view.View".into(),
        ..spacer
    };
    assert!(compress(&screen(vec![spacer])).opaque.is_empty());
}
