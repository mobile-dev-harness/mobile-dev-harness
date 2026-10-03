//! The verification engine end to end against a scripted fake driver: no device needed.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mdh_control::{Control, Session};
use mdh_core::output::Timings;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource};
use mdh_core::{
    Device, DeviceState, Error, Input, LaunchInfo, LogEntry, LogLevel, Platform, Result,
};
use mdh_driver::Driver;
use mdh_verify::{Assertion, Flow, FlowOptions, Status, VerifyOptions, run_flow, verify};

/// Serves scripted screens in order (repeating the last) and records what was done.
#[derive(Default)]
struct FakeDriver {
    screens: Mutex<VecDeque<(&'static str, Vec<RawNode>)>>,
    done: Mutex<Vec<String>>,
    logs: Mutex<Vec<LogEntry>>,
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
            roots: self.current(false).1,
            source: TreeSource::Helper,
            windows: Vec::new(),
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
        Ok(LaunchInfo {
            activity: None,
            total_time_ms: 100,
            reused_existing: false,
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
}

fn device() -> Device {
    Device {
        id: "fake-1".into(),
        platform: Platform::Android,
        state: DeviceState::Online,
        model: None,
        is_emulator: true,
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
