//! The verification engine end to end against a scripted fake driver: no device needed.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mdh_control::{Control, Session};
use mdh_core::output::Timings;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource, WindowInfo, WindowKind};
use mdh_core::{
    Appearance, AppearanceKind, Device, DeviceState, Error, Input, LaunchInfo, LogEntry, LogLevel,
    Platform, Result,
};
use mdh_driver::Driver;
use mdh_verify::{Assertion, Flow, FlowOptions, Status, VerifyOptions, run_flow, verify};

/// Serves scripted screens in order (repeating the last) and records what was done.
#[derive(Default)]
struct FakeDriver {
    screens: Mutex<VecDeque<(&'static str, Vec<RawNode>)>>,
    done: Mutex<Vec<String>>,
    logs: Mutex<Vec<LogEntry>>,
    /// The screen can't be read once the app is launched (uiautomator printed no hierarchy).
    unreadable: Mutex<bool>,
    launched: Mutex<bool>,
    /// Appearance settings changed so far, the latest last.
    settings: Mutex<Vec<Appearance>>,
    /// The keyboard's window and for how many more reads of the screen it stays up.
    keyboard: Mutex<Option<(Rect, usize)>>,
}

impl FakeDriver {
    fn script(&self, screens: impl IntoIterator<Item = (&'static str, Vec<RawNode>)>) {
        *self.screens.lock().unwrap() = screens.into_iter().collect();
    }

    fn current(&self, advance: bool) -> (&'static str, Vec<RawNode>) {
        let mut s = self.screens.lock().unwrap();
        if advance && s.len() > 1 {
            s.pop_front();
        }
        s.front()
            .cloned()
            .unwrap_or(("com.example/.Main", Vec::new()))
    }

    fn did(&self, what: impl Into<String>) {
        self.done.lock().unwrap().push(what.into());
    }

    /// The windows on screen at this read: none are listed unless the keyboard is up.
    fn windows(&self) -> Vec<WindowInfo> {
        let mut keyboard = self.keyboard.lock().unwrap();
        let Some((bounds, reads)) = *keyboard else {
            return Vec::new();
        };
        *keyboard = (reads > 1).then(|| (bounds, reads - 1));
        if keyboard.is_none() {
            self.did("keyboard hidden");
        }
        let window = |kind, active, bounds| WindowInfo {
            kind,
            active,
            focused: active,
            title: None,
            package: None,
            bounds,
        };
        vec![
            window(WindowKind::InputMethod, false, bounds),
            window(WindowKind::Application, true, Rect::new(0, 0, 1000, 2000)),
        ]
    }
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
        if *self.unreadable.lock().unwrap() && *self.launched.lock().unwrap() {
            return Err(Error::Parse {
                tool: "uiautomator".into(),
                detail: "no <hierarchy> element".into(),
            });
        }
        Ok(RawTree {
            roots: self.current(false).1,
            source: TreeSource::Helper,
            windows: self.windows(),
        })
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(Some(self.current(false).0.into()))
    }
    async fn clock_ms(&self, _: &Device) -> Result<u64> {
        Ok(1_000)
    }
    async fn logs(&self, _: &Device, since_ms: u64) -> Result<Vec<LogEntry>> {
        Ok(self
            .logs
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.time_ms > since_ms)
            .cloned()
            .collect())
    }
    async fn pids(&self, _: &Device, _: &[String]) -> Result<Vec<u32>> {
        Ok(vec![4321])
    }
    /// Every input moves to the next scripted screen.
    async fn input(&self, _: &Device, input: &Input) -> Result<()> {
        self.did(format!("{input:?}"));
        self.current(true);
        Ok(())
    }
    async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
        Err(Error::Unsupported {
            operation: "screenshots".into(),
        })
    }
    async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
        Ok(())
    }
    async fn launch(&self, _: &Device, app: &str) -> Result<LaunchInfo> {
        self.did(format!("launch {app}"));
        *self.launched.lock().unwrap() = true;
        Ok(LaunchInfo {
            activity: None,
            total_time_ms: 100,
            reused_existing: false,
            state: None,
        })
    }
    async fn stop(&self, _: &Device, package: &str) -> Result<()> {
        self.did(format!("stop {package}"));
        Ok(())
    }
    async fn clear_data(&self, _: &Device, package: &str) -> Result<()> {
        self.did(format!("clear {package}"));
        Ok(())
    }
    /// What was set last, or the defaults of a fresh emulator.
    async fn appearance(&self, _: &Device, kind: &AppearanceKind) -> Result<Appearance> {
        let settings = self.settings.lock().unwrap();
        if let Some(set) = settings.iter().rev().find(|a| a.kind() == *kind) {
            return Ok(set.clone());
        }
        Ok(match kind {
            AppearanceKind::NightMode => Appearance::NightMode("no".into()),
            AppearanceKind::TimeZone => Appearance::TimeZone("America/Los_Angeles".into()),
            AppearanceKind::Rotation => Appearance::Rotation {
                auto: true,
                user: 0,
            },
            other => panic!("not scripted: {other:?}"),
        })
    }
    async fn set_appearance(&self, _: &Device, value: &Appearance) -> Result<()> {
        self.did(format!("set {value:?}"));
        self.settings.lock().unwrap().push(value.clone());
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

fn node(class: &str, id: Option<&str>, text: Option<&str>, top: i32, flags: NodeFlags) -> RawNode {
    RawNode {
        class: class.into(),
        resource_id: id.map(|i| format!("com.example:id/{i}")),
        text: text.map(str::to_owned),
        bounds: Rect::new(0, top, 1000, top + 100),
        flags,
        ..RawNode::default()
    }
}

fn root(children: Vec<RawNode>) -> Vec<RawNode> {
    vec![RawNode {
        class: "android.widget.FrameLayout".into(),
        bounds: Rect::new(0, 0, 1000, 2000),
        flags: enabled(),
        children,
        ..RawNode::default()
    }]
}

fn enabled() -> NodeFlags {
    NodeFlags {
        enabled: true,
        ..NodeFlags::default()
    }
}

fn button(id: &str, text: &str, top: i32, on: bool) -> RawNode {
    node(
        "android.widget.Button",
        Some(id),
        Some(text),
        top,
        NodeFlags {
            clickable: true,
            enabled: on,
            ..NodeFlags::default()
        },
    )
}

fn login(sign_in_enabled: bool) -> (&'static str, Vec<RawNode>) {
    (
        "com.example/.LoginActivity",
        root(vec![
            node(
                "android.widget.TextView",
                Some("title"),
                Some("Welcome"),
                100,
                enabled(),
            ),
            button("sign_in", "Sign in", 300, sign_in_enabled),
        ]),
    )
}

fn inbox() -> (&'static str, Vec<RawNode>) {
    (
        "com.example/.InboxActivity",
        root(vec![node(
            "android.widget.TextView",
            Some("title"),
            Some("Inbox"),
            100,
            enabled(),
        )]),
    )
}

fn options() -> VerifyOptions {
    VerifyOptions {
        timeout: Duration::from_millis(300),
        runs: None,
        ..VerifyOptions::default()
    }
}

fn checks(list: &[&str]) -> Vec<Assertion> {
    list.iter().map(|c| Assertion::parse(c).unwrap()).collect()
}

#[tokio::test]
async fn a_verdict_shows_what_was_observed_for_each_failed_check() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([login(false)]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let verdict = verify(
        &mut session,
        checks(&[
            "screen .LoginActivity",
            r#"text id=title == "Welcome""#,
            "enabled id=sign_in",
            "visible Inbox",
            "not visible id=error",
        ]),
        &options(),
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Fail);
    let text = verdict.text.split_once('\n').unwrap().1;
    assert_eq!(
        text,
        r#"  ✓ screen .LoginActivity
  ✓ text id=title == "Welcome"
  ✗ enabled id=sign_in — disabled: [e2] button "Sign in" disabled #sign_in
  ✗ visible "Inbox" — not on screen
  ✓ not visible id=error
  ✓ no crash"#
    );
    assert!(matches!(
        verdict.failure(),
        Some(Error::VerificationFailed {
            failed: 2,
            total: 6
        })
    ));
}

#[tokio::test]
async fn a_crash_earlier_in_the_session_fails_no_crash() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([inbox()]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    session
        .observe(false, &mut Timings::default())
        .await
        .unwrap(); // starts watching at 1000
    let entry = |message: &str| LogEntry {
        time_ms: 2_000,
        pid: 4321,
        tid: 4321,
        level: LogLevel::Error,
        tag: "AndroidRuntime".into(),
        message: message.into(),
    };
    *driver.logs.lock().unwrap() = vec![
        entry("FATAL EXCEPTION: main"),
        entry("Process: com.example, PID: 4321"),
        entry("java.lang.IllegalStateException: boom"),
        entry("\tat com.example.Inbox.load(Inbox.kt:12)"),
    ];
    // The crash was already shown after an action; the verdict still fails.
    session
        .observe(false, &mut Timings::default())
        .await
        .unwrap();
    let verdict = verify(
        &mut session,
        checks(&["visible Inbox"]),
        &options(),
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Fail);
    assert!(
        verdict
            .text
            .contains("✗ no crash — java.lang.IllegalStateException: boom"),
        "{}",
        verdict.text
    );
    assert!(
        verdict
            .text
            .contains("at com.example.Inbox.load(Inbox.kt:12)"),
        "{}",
        verdict.text
    );
}

const FLOW: &str = r#"
name: sign-in
app: com.example
setup:
  reset: data
steps:
  - tap: id=sign_in
assert:
  - screen .InboxActivity
  - visible Inbox
"#;

#[tokio::test]
async fn a_flow_restarts_the_app_replays_and_checks() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([login(true), inbox()]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let flow = Flow::parse("sign-in", FLOW).unwrap();
    let verdict = run_flow(
        &mut session,
        &flow,
        &FlowOptions {
            verify: options(),
            step_timeout: Duration::from_millis(500),
        },
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Pass, "{}", verdict.text);
    assert!(
        verdict
            .text
            .starts_with("verdict sign-in: PASS · 1 step · 3 of 3 checks passed"),
        "{}",
        verdict.text
    );
    let done = driver.done.lock().unwrap().clone();
    assert_eq!(
        done[..3],
        [
            "clear com.example",
            "stop com.example",
            "launch com.example"
        ]
    );
    assert!(done[3].starts_with("Tap"), "{done:?}");
}

#[tokio::test]
async fn a_step_that_cannot_run_stops_the_flow_and_says_why() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([inbox()]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let flow = Flow::parse("sign-in", FLOW).unwrap();
    let verdict = run_flow(
        &mut session,
        &flow,
        &FlowOptions {
            verify: options(),
            step_timeout: Duration::from_millis(300),
        },
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Fail);
    assert!(
        verdict
            .text
            .contains("✗ step 1: tap id=sign_in — not on screen after 0 s (screen .InboxActivity)"),
        "{}",
        verdict.text
    );
    assert!(
        verdict.text.contains("stopped after 0 of 1 steps"),
        "{}",
        verdict.text
    );
    // The final checks don't run after a failed step; `no crash` does.
    assert_eq!(verdict.findings.len(), 2, "{}", verdict.text);
}

/// The keyboard is still on its way out when the next step wants the button under it. The
/// button is in the tree all along (obscured); the step waits for it like for one that isn't
/// there yet.
#[tokio::test]
async fn a_step_waits_for_its_target_to_come_out_from_under_the_keyboard() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([login(true), inbox()]);
    *driver.keyboard.lock().unwrap() = Some((Rect::new(0, 250, 1000, 2000), 6));
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let flow = Flow::parse("sign-in", FLOW).unwrap();
    let verdict = run_flow(
        &mut session,
        &flow,
        &FlowOptions {
            verify: options(),
            step_timeout: Duration::from_secs(5),
        },
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Pass, "{}", verdict.text);
    let done = driver.done.lock().unwrap().clone();
    let at = |what: &str| done.iter().position(|d| d.starts_with(what)).unwrap();
    assert!(at("keyboard hidden") < at("Tap"), "{done:?}");
}

#[tokio::test]
async fn a_step_whose_target_stays_under_the_keyboard_fails_saying_so() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([login(true), inbox()]);
    *driver.keyboard.lock().unwrap() = Some((Rect::new(0, 250, 1000, 2000), usize::MAX));
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let flow = Flow::parse("sign-in", FLOW).unwrap();
    let verdict = run_flow(
        &mut session,
        &flow,
        &FlowOptions {
            verify: options(),
            step_timeout: Duration::from_millis(300),
        },
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Fail, "{}", verdict.text);
    assert!(
        verdict.text.contains(
            "✗ step 1: tap id=sign_in — on screen but covered by system windows: \
             [e2] button \"Sign in\" obscured #sign_in after 0 s (screen .LoginActivity)"
        ),
        "{}",
        verdict.text
    );
    assert!(
        !driver
            .done
            .lock()
            .unwrap()
            .iter()
            .any(|d| d.starts_with("Tap")),
        "nothing was tapped"
    );
}

/// Seen in the benchmark: the grader couldn't read the screen, and a correct fix was judged broken.
#[tokio::test]
async fn an_unreadable_screen_is_an_error_not_a_failure_of_the_app() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([login(true), inbox()]);
    *driver.unreadable.lock().unwrap() = true;
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let flow = Flow::parse("sign-in", FLOW).unwrap();
    let verdict = run_flow(
        &mut session,
        &flow,
        &FlowOptions {
            verify: options(),
            step_timeout: Duration::from_millis(300),
        },
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Error, "{}", verdict.text);
    assert!(
        verdict.text.starts_with("verdict sign-in: ERROR"),
        "{}",
        verdict.text
    );
    assert!(
        verdict.text.contains("no <hierarchy> element"),
        "{}",
        verdict.text
    );
}

const UNDER_SETTINGS: &str = r#"
name: tokyo-night
app: com.example
setup:
  device:
    dark: true
    time_zone: Asia/Tokyo
    orientation: landscape
steps:
  - tap: id=sign_in
assert:
  - visible Inbox
"#;

#[tokio::test]
async fn a_flow_runs_under_its_device_settings_and_puts_them_back() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([login(true), inbox()]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let flow = Flow::parse("tokyo-night", UNDER_SETTINGS).unwrap();
    let verdict = run_flow(
        &mut session,
        &flow,
        &FlowOptions {
            verify: options(),
            step_timeout: Duration::from_millis(500),
        },
        &mut Timings::default(),
    )
    .await
    .unwrap();
    assert_eq!(verdict.status, Status::Pass, "{}", verdict.text);
    let done = driver.done.lock().unwrap().clone();
    let sets: Vec<&str> = done
        .iter()
        .map(String::as_str)
        .filter(|d| d.starts_with("set ") || d.starts_with("launch"))
        .collect();
    assert_eq!(
        sets,
        [
            r#"set NightMode("yes")"#,
            r#"set TimeZone("Asia/Tokyo")"#,
            "set Rotation { auto: false, user: 1 }",
            "launch com.example",
            "set Rotation { auto: true, user: 0 }",
            r#"set TimeZone("America/Los_Angeles")"#,
            r#"set NightMode("no")"#,
        ]
    );
    // Written back the way it was read.
    assert!(
        flow.to_yaml().contains("time_zone: Asia/Tokyo"),
        "{}",
        flow.to_yaml()
    );
}

#[test]
fn device_settings_are_checked_when_the_flow_is_read() {
    let ok = Flow::parse(
        "tablet",
        "name: tablet\napp: com.example\nsetup:\n  device:\n    font_scale: 1.3\n    display: 1600x2560@320\nsteps: []\n",
    )
    .unwrap();
    assert!(ok.to_yaml().contains("font_scale: 1.3"), "{}", ok.to_yaml());
    assert!(
        ok.to_yaml().contains("display: 1600x2560@320"),
        "{}",
        ok.to_yaml()
    );
    for (bad, says) in [
        ("display: 1600", "WIDTHxHEIGHT"),
        ("font_scale: 9", "0.5 to 3"),
        ("night: true", "unknown field"),
    ] {
        let err = Flow::parse(
            "bad",
            &format!("name: bad\nsetup:\n  device:\n    {bad}\nsteps: []\n"),
        )
        .unwrap_err();
        assert!(err.to_string().contains(says), "{bad}: {err}");
    }
}

#[test]
fn a_change_selects_the_flows_that_pass_its_screens() {
    let dir = std::env::temp_dir().join(format!("mdh-flows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = mdh_verify::FlowStore::new(&dir);
    for (name, screens) in [
        ("login", "[LoginActivity, InboxActivity]"),
        ("settings", "[SettingsActivity]"),
    ] {
        let yaml = format!("name: {name}\nscreens: {screens}\nsteps:\n- key: back\n");
        store.save(&Flow::parse(name, &yaml).unwrap()).unwrap();
    }
    let screen = |name: &str, host: Option<&str>| mdh_impact::ScreenImpact {
        screen: name.into(),
        kind: mdh_impact::ScreenKind::Activity,
        file: String::new(),
        via: Vec::new(),
        confidence: mdh_impact::Confidence::Exact,
        host: host.map(str::to_owned),
        changes: 1,
        reach: Vec::new(),
    };
    let mut report = mdh_impact::ImpactReport {
        screens: vec![screen("InboxScreen", Some("InboxActivity"))],
        ..Default::default()
    };
    assert_eq!(mdh_verify::flows_for(&report, &store).unwrap(), ["login"]);
    report.screens = vec![screen("AboutActivity", None)];
    assert!(mdh_verify::flows_for(&report, &store).unwrap().is_empty());
    report.other_files.push(mdh_impact::OtherFile {
        path: "app/build.gradle.kts".into(),
        status: mdh_impact::FileStatus::Modified,
        kind: "build",
    });
    assert_eq!(
        mdh_verify::flows_for(&report, &store).unwrap(),
        ["login", "settings"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
