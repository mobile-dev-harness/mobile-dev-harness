//! UI consistency checks against scripted screens: rules on the tree, structural baselines.

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mdh_control::{Control, Session};
use mdh_core::output::Timings;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource};
use mdh_core::{
    Appearance, AppearanceKind, Device, DeviceState, Error, Input, LaunchInfo, Platform, Result,
};
use mdh_driver::Driver;
use mdh_verify::{Check, CheckContext, Outcome};
use mdh_visual::Visual;

/// Serves one screen, replaceable between checks; density 480 (3 px per dp).
struct FakeDriver {
    screen: Mutex<Vec<RawNode>>,
    /// What `screenshot` returns, as PNG.
    png: Mutex<Option<Vec<u8>>>,
    /// Appearance settings written, in order; a large font scale hides `#more`.
    appearance: Mutex<Vec<Appearance>>,
}

#[async_trait]
impl Driver for FakeDriver {
    fn platform(&self) -> Platform {
        Platform::Android
    }
    async fn devices(&self) -> Result<Vec<Device>> {
        Ok(vec![device()])
    }
    async fn ui_tree(&self, _: &Device) -> Result<RawTree> {
        Ok(RawTree {
            roots: self.screen.lock().unwrap().clone(),
            source: TreeSource::Helper,
            windows: Vec::new(),
        })
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(Some("com.example/.Main".into()))
    }
    async fn density(&self, _: &Device) -> Result<u32> {
        Ok(480)
    }
    async fn appearance(&self, _: &Device, kind: &AppearanceKind) -> Result<Appearance> {
        Ok(match kind {
            AppearanceKind::FontScale => Appearance::FontScale(None),
            AppearanceKind::NightMode => Appearance::NightMode("no".into()),
            AppearanceKind::AppLocales { package } => Appearance::AppLocales {
                package: package.clone(),
                locales: String::new(),
            },
            AppearanceKind::Rotation => Appearance::Rotation {
                auto: true,
                user: 0,
            },
            AppearanceKind::Display => Appearance::Display {
                size: None,
                density: None,
            },
            AppearanceKind::TimeZone => Appearance::TimeZone("America/Los_Angeles".into()),
        })
    }
    async fn set_appearance(&self, _: &Device, value: &Appearance) -> Result<()> {
        self.appearance.lock().unwrap().push(value.clone());
        let mut screen = self.screen.lock().unwrap();
        let root = &mut screen[0].children;
        let has_more = root
            .iter()
            .any(|n| n.resource_id.as_deref() == Some("com.example:id/more"));
        match value {
            Appearance::FontScale(Some(_)) => {
                root.retain(|n| n.resource_id.as_deref() != Some("com.example:id/more"))
            }
            Appearance::FontScale(None) if !has_more => root.push(control(
                "android.widget.Button",
                "more",
                Some("More"),
                Rect::new(0, 900, 1080, 1050),
            )),
            _ => {}
        }
        Ok(())
    }
    async fn input(&self, _: &Device, _: &Input) -> Result<()> {
        Ok(())
    }
    async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
        self.png.lock().unwrap().clone().ok_or(Error::Unsupported {
            operation: "screenshots".into(),
        })
    }
    async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
        Ok(())
    }
    async fn launch(&self, _: &Device, _: &str) -> Result<LaunchInfo> {
        unreachable!()
    }
    async fn stop(&self, _: &Device, _: &str) -> Result<()> {
        Ok(())
    }
}

fn device() -> Device {
    Device {
        id: "fake-1".into(),
        platform: Platform::Android,
        state: DeviceState::Online,
        model: None,
        is_emulator: true,
        avd: None,
        api: None,
        manufacturer: None,
    }
}

fn control(class: &str, id: &str, text: Option<&str>, bounds: Rect) -> RawNode {
    RawNode {
        class: class.into(),
        resource_id: Some(format!("com.example:id/{id}")),
        text: text.map(str::to_owned),
        bounds,
        flags: NodeFlags {
            clickable: true,
            enabled: true,
            ..NodeFlags::default()
        },
        ..RawNode::default()
    }
}

fn root(children: Vec<RawNode>) -> Vec<RawNode> {
    vec![RawNode {
        class: "android.widget.FrameLayout".into(),
        bounds: Rect::new(0, 0, 1080, 2400),
        flags: NodeFlags {
            enabled: true,
            ..NodeFlags::default()
        },
        children,
        ..RawNode::default()
    }]
}

async fn run(
    driver: &Arc<FakeDriver>,
    dir: &Path,
    config: serde_json::Value,
) -> Vec<(Outcome, String)> {
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let mut timings = Timings::default();
    let mut cx = CheckContext {
        session: &mut session,
        run_dir: None,
        step: None,
        since_ms: None,
        scope: Some("login"),
        checkpoint: "final",
        config: Some(&config),
        timings: &mut timings,
    };
    let check = Visual {
        baselines: dir.to_owned(),
    };
    check
        .run(&mut cx)
        .await
        .unwrap()
        .into_iter()
        .map(|f| {
            let mut line = f.check;
            if let Some(o) = f.observed {
                line.push_str(&format!(" — {o}"));
            }
            for e in f.evidence {
                line.push_str(&format!(" | {e}"));
            }
            (f.outcome, line)
        })
        .collect()
}

#[tokio::test]
async fn rules_find_small_unlabeled_and_overlapping_controls() {
    let driver = Arc::new(FakeDriver {
        png: Mutex::new(None),
        appearance: Mutex::default(),
        screen: Mutex::new(root(vec![
            // 360×150 px at 480 dpi = 120×50 dp: fine.
            control(
                "android.widget.Button",
                "ok",
                Some("OK"),
                Rect::new(0, 100, 360, 250),
            ),
            // 36×36 dp and no label.
            control(
                "android.widget.ImageButton",
                "share",
                None,
                Rect::new(500, 100, 608, 208),
            ),
            // Overlaps the OK button by half.
            control(
                "android.widget.Button",
                "cancel",
                Some("Cancel"),
                Rect::new(180, 100, 540, 250),
            ),
        ])),
    });
    let dir = std::env::temp_dir().join(format!("mdh-visual-rules-{}", std::process::id()));
    let findings = run(&driver, &dir, serde_json::json!({ "rules": "all" })).await;
    let lines: Vec<String> = findings.iter().map(|(o, l)| format!("{o:?} {l}")).collect();
    assert_eq!(
        lines,
        [
            "Fail ui: touch targets ≥ 48 dp — [e2] button #share: 36×36 dp, needs 48×48",
            "Fail ui: controls have labels — [e2] button #share: no label; screen readers can't name it",
            r#"Fail ui: controls don't overlap — 2 elements; [e1] button "OK" #ok: overlaps [e3] button "Cancel" #cancel | [e2] button #share: overlaps [e3] button "Cancel" #cancel"#,
        ],
    );
    // Without a `visual:` section the same rules only warn.
    let findings = run(&driver, &dir, serde_json::json!({})).await;
    assert!(
        findings.iter().all(|(o, _)| *o == Outcome::Warn),
        "{findings:?}"
    );
}

#[tokio::test]
async fn baselines_are_recorded_compared_and_approved() {
    let screen = |sign_in_top: i32, label: &str| {
        root(vec![
            control(
                "android.widget.TextView",
                "title",
                Some("Welcome"),
                Rect::new(0, 100, 1080, 200),
            ),
            control(
                "android.widget.Button",
                "sign_in",
                Some(label),
                Rect::new(0, sign_in_top, 1080, sign_in_top + 150),
            ),
        ])
    };
    let driver = Arc::new(FakeDriver {
        screen: Mutex::new(screen(600, "Sign in")),
        png: Mutex::new(None),
        appearance: Mutex::default(),
    });
    let dir = std::env::temp_dir().join(format!("mdh-visual-baseline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = serde_json::json!({ "baseline": true, "rules": "none", "pixels": false });

    let first = run(&driver, &dir, config.clone()).await;
    assert_eq!(first[0].0, Outcome::Warn, "{first:?}");
    assert!(first[0].1.contains("no baseline yet"), "{first:?}");

    // 6 px (2 dp) is within the tolerance.
    *driver.screen.lock().unwrap() = screen(606, "Sign in");
    let same = run(&driver, &dir, config.clone()).await;
    assert_eq!(
        same,
        [(Outcome::Pass, "ui matches baseline login/final".to_owned())]
    );

    *driver.screen.lock().unwrap() = screen(744, "Log in");
    let moved = run(&driver, &dir, config.clone()).await;
    assert_eq!(moved[0].0, Outcome::Fail);
    assert_eq!(
        moved[0].1,
        r#"ui matches baseline login/final — 1 deviation: ~ button #sign_in: text "Sign in" → "Log in", moved down 48 dp | intended? `mdh visual approve login` makes this the baseline"#
    );

    let store = mdh_visual::baseline::Store { dir: dir.clone() };
    assert_eq!(store.approve(Some("login")).unwrap().len(), 1);
    let approved = run(&driver, &dir, config).await;
    assert_eq!(approved[0].0, Outcome::Pass, "{approved:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn png(paint: Option<(Rect, [u8; 3])>) -> Vec<u8> {
    let mut img = image::RgbImage::from_pixel(1080, 2400, image::Rgb([255, 255, 255]));
    if let Some((r, color)) = paint {
        for y in r.top..r.bottom {
            for x in r.left..r.right {
                img.put_pixel(x as u32, y as u32, image::Rgb(color));
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
async fn pixel_baselines_name_the_element_that_changed() {
    let button = Rect::new(0, 600, 1080, 750);
    let driver = Arc::new(FakeDriver {
        screen: Mutex::new(root(vec![control(
            "android.widget.Button",
            "sign_in",
            Some("Sign in"),
            button,
        )])),
        png: Mutex::new(Some(png(Some((button, [30, 100, 200]))))),
        appearance: Mutex::default(),
    });
    let dir = std::env::temp_dir().join(format!("mdh-visual-pixels-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = serde_json::json!({ "baseline": true, "rules": "none" });

    let first = run(&driver, &dir, config.clone()).await;
    assert_eq!(
        first.iter().filter(|(o, _)| *o == Outcome::Warn).count(),
        2,
        "{first:?}"
    );
    let same = run(&driver, &dir, config.clone()).await;
    assert!(same.iter().all(|(o, _)| *o == Outcome::Pass), "{same:?}");

    // The button turns red: the tree is the same, the pixels aren't.
    *driver.png.lock().unwrap() = Some(png(Some((button, [200, 30, 30]))));
    let changed = run(&driver, &dir, config.clone()).await;
    let (outcome, line) = changed
        .iter()
        .find(|(_, l)| l.starts_with("ui pixels"))
        .unwrap();
    assert_eq!(*outcome, Outcome::Fail);
    // Regions are whole 16 px blocks, so they start a little above the button.
    assert!(
        line.contains("1 region changed: 360×53 dp at 0,197"),
        "{line}"
    );
    assert!(
        line.contains(r#"in [e1] button "Sign in" #sign_in"#),
        "{line}"
    );

    // A mask over the button hides the change.
    let masked =
        serde_json::json!({ "baseline": true, "rules": "none", "mask": [[0, 196, 360, 56]] });
    let hidden = run(&driver, &dir, masked).await;
    assert!(
        hidden.iter().all(|(o, _)| *o == Outcome::Pass),
        "{hidden:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn other_configurations_find_what_goes_missing_and_restore_the_setting() {
    let driver = Arc::new(FakeDriver {
        screen: Mutex::new(root(vec![
            control(
                "android.widget.Button",
                "ok",
                Some("OK"),
                Rect::new(0, 100, 1080, 250),
            ),
            control(
                "android.widget.Button",
                "more",
                Some("More"),
                Rect::new(0, 900, 1080, 1050),
            ),
        ])),
        png: Mutex::new(None),
        appearance: Mutex::default(),
    });
    let dir = std::env::temp_dir().join(format!("mdh-visual-configs-{}", std::process::id()));
    let findings = run(
        &driver,
        &dir,
        serde_json::json!({ "rules": "none", "configs": ["font_scale", "dark"] }),
    )
    .await;
    assert_eq!(
        findings,
        [
            (
                Outcome::Fail,
                "ui at font scale 1.3 — missing compared with the default configuration: #more"
                    .to_owned()
            ),
            (Outcome::Pass, "ui in dark mode: layout holds".to_owned()),
        ]
    );
    // Each setting was switched and put back.
    assert_eq!(
        *driver.appearance.lock().unwrap(),
        [
            Appearance::FontScale(Some("1.3".into())),
            Appearance::FontScale(None),
            Appearance::NightMode("yes".into()),
            Appearance::NightMode("no".into()),
        ]
    );
}
