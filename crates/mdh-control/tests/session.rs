//! The session engine end to end against a scripted fake driver: no device needed.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mdh_control::{Action, Control, Session, Target};
use mdh_core::output::Timings;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource};
use mdh_core::{
    Device, DeviceState, Error, Input, LaunchInfo, LogEntry, LogLevel, Platform, Result,
};
use mdh_driver::Driver;

/// Serves scripted trees in order (repeating the last one) and records input.
#[derive(Default)]
struct FakeDriver {
    trees: Mutex<VecDeque<Vec<RawNode>>>,
    inputs: Mutex<Vec<Input>>,
    /// Handed out by the next `logs` call.
    logs: Mutex<Vec<LogEntry>>,
}

impl FakeDriver {
    fn script(&self, screens: impl IntoIterator<Item = Vec<RawNode>>) {
        *self.trees.lock().unwrap() = screens.into_iter().collect();
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
        let mut trees = self.trees.lock().unwrap();
        let roots = if trees.len() > 1 {
            trees.pop_front().unwrap()
        } else {
            trees.front().cloned().unwrap_or_default()
        };
        Ok(RawTree {
            roots,
            source: TreeSource::Helper,
            windows: Vec::new(),
        })
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(Some("com.example/.Settings".into()))
    }
    async fn clock_ms(&self, _: &Device) -> Result<u64> {
        Ok(1_000)
    }
    async fn logs(&self, _: &Device, since_ms: u64) -> Result<Vec<LogEntry>> {
        let mut logs = self.logs.lock().unwrap();
        Ok(std::mem::take(&mut *logs)
            .into_iter()
            .filter(|e| e.time_ms > since_ms)
            .collect())
    }
    async fn pids(&self, _: &Device, _: &[String]) -> Result<Vec<u32>> {
        Ok(vec![4321])
    }
    async fn input(&self, _: &Device, input: &Input) -> Result<()> {
        self.inputs.lock().unwrap().push(input.clone());
        Ok(())
    }
    async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
        unsupported()
    }
    async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
        unsupported()
    }
    async fn launch(&self, _: &Device, _: &str) -> Result<LaunchInfo> {
        unsupported()
    }
    async fn stop(&self, _: &Device, _: &str) -> Result<()> {
        unsupported()
    }
}

fn unsupported<T>() -> Result<T> {
    Err(Error::HelperUnavailable {
        reason: "not scripted".into(),
    })
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
    }
}

/// A settings screen with a Wi-Fi switch row whose state is `on`; `shift` moves it (animation).
fn screen(on: bool, shift: i32) -> Vec<RawNode> {
    let enabled = NodeFlags {
        enabled: true,
        ..NodeFlags::default()
    };
    let row = RawNode {
        class: "android.widget.LinearLayout".into(),
        bounds: Rect::new(0, 200 + shift, 1000, 300 + shift),
        flags: NodeFlags {
            clickable: true,
            ..enabled
        },
        children: vec![
            RawNode {
                class: "android.widget.TextView".into(),
                text: Some("Wi-Fi".into()),
                bounds: Rect::new(50, 200 + shift, 500, 300 + shift),
                flags: enabled,
                ..RawNode::default()
            },
            RawNode {
                class: "android.widget.Switch".into(),
                bounds: Rect::new(800, 200 + shift, 950, 300 + shift),
                flags: NodeFlags {
                    clickable: true,
                    checkable: true,
                    checked: on,
                    ..enabled
                },
                ..RawNode::default()
            },
        ],
        ..RawNode::default()
    };
    vec![RawNode {
        class: "android.widget.FrameLayout".into(),
        bounds: Rect::new(0, 0, 1000, 2000),
        flags: enabled,
        children: vec![row],
        ..RawNode::default()
    }]
}

#[tokio::test]
async fn tap_waits_for_the_ui_to_settle_and_reports_the_diff() {
    let driver = Arc::new(FakeDriver::default());
    let session_file =
        std::env::temp_dir().join(format!("mdh-session-test-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&session_file);
    let mut timings = Timings::default();

    driver.script([screen(false, 0)]);
    let mut session = Session::open(
        Control::new(driver.clone(), device()),
        Some(session_file.clone()),
    );
    let observed = session.observe(false, &mut timings).await.unwrap();
    assert!(
        observed.text.contains(r#"[e1] switch "Wi-Fi" off"#),
        "{}",
        observed.text
    );

    // Before the tap, then two animation frames, then the settled screen.
    driver.script([
        screen(false, 0),
        screen(true, 40),
        screen(true, 20),
        screen(true, 0),
    ]);
    let outcome = session
        .act(
            Action::Tap {
                target: Target::Ref("e1".into()),
            },
            &mut timings,
        )
        .await
        .unwrap();

    assert_eq!(
        driver.inputs.lock().unwrap().as_slice(),
        [Input::Tap { x: 500, y: 250 }],
        "tapped the center of the row"
    );
    assert!(outcome.settled);
    assert!(!outcome.new_screen);
    assert!(
        outcome.text.ends_with(r#"~ [e1] switch "Wi-Fi": off → on"#),
        "{}",
        outcome.text
    );

    // The recording is replayable: the ref became a selector.
    session.save().unwrap();
    let reopened = Session::open(
        Control::new(driver.clone(), device()),
        Some(session_file.clone()),
    );
    let step = &reopened.steps()[0];
    assert_eq!(
        step.action,
        Action::Tap {
            target: Target::parse("role=switch;text=Wi-Fi").unwrap()
        }
    );
    std::fs::remove_file(session_file).unwrap();
}

#[tokio::test]
async fn missing_targets_fail_without_input() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([screen(false, 0)]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let err = session
        .act(
            Action::Tap {
                target: Target::parse("Bluetooth").unwrap(),
            },
            &mut Timings::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::ElementNotFound { .. }), "{err}");
    assert!(driver.inputs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_crash_during_an_action_is_reported_with_the_steps_before_it() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([screen(false, 0)]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let mut timings = Timings::default();
    session.observe(false, &mut timings).await.unwrap(); // starts the log cursor at 1000

    let entry = |ms, level, tag: &str, message: &str| LogEntry {
        time_ms: ms,
        pid: 4321,
        tid: 4321,
        level,
        tag: tag.into(),
        message: message.into(),
    };
    *driver.logs.lock().unwrap() = vec![
        entry(
            900,
            LogLevel::Error,
            "Old",
            "before the session; not reported",
        ),
        entry(
            2_000,
            LogLevel::Error,
            "AndroidRuntime",
            "FATAL EXCEPTION: main",
        ),
        entry(
            2_000,
            LogLevel::Error,
            "AndroidRuntime",
            "Process: com.example, PID: 4321",
        ),
        entry(
            2_000,
            LogLevel::Error,
            "AndroidRuntime",
            "java.lang.IllegalStateException: boom",
        ),
        entry(
            2_000,
            LogLevel::Error,
            "AndroidRuntime",
            "\tat com.example.Settings.onToggle(Settings.kt:42)",
        ),
    ];
    let outcome = session
        .act(
            Action::Tap {
                target: Target::parse("Wi-Fi").unwrap(),
            },
            &mut timings,
        )
        .await
        .unwrap();

    let logs = outcome.logs.expect("a crash is reported");
    assert_eq!(
        logs.errors, 0,
        "crash lines are not double-counted as errors"
    );
    assert_eq!(logs.crashes.len(), 1);
    assert!(logs.crashes[0].of_app);
    assert!(
        outcome
            .text
            .contains("!! CRASH com.example (pid 4321): java.lang.IllegalStateException: boom"),
        "{}",
        outcome.text
    );
    assert!(
        outcome
            .text
            .contains("at com.example.Settings.onToggle(Settings.kt:42)")
    );
    assert!(
        outcome.text.contains(r#"after: tap e1 switch "Wi-Fi""#),
        "{}",
        outcome.text
    );
}
