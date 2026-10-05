//! The session engine end to end against a scripted fake driver: no device needed.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mdh_control::{Action, Control, Direction, Session, Target};
use mdh_core::output::Timings;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource, WindowInfo, WindowKind};
use mdh_core::{
    Device, DeviceState, Error, Input, LaunchInfo, LogEntry, LogLevel, Platform, Result,
};
use mdh_driver::Driver;

/// Serves scripted trees in order (repeating the last one) and records input.
#[derive(Default)]
struct FakeDriver {
    trees: Mutex<VecDeque<Vec<RawNode>>>,
    /// The windows on screen at each read, in step with the trees (repeating the last list).
    windows: Mutex<VecDeque<Vec<WindowInfo>>>,
    inputs: Mutex<Vec<Input>>,
    /// Handed out by the next `logs` call.
    logs: Mutex<Vec<LogEntry>>,
}

impl FakeDriver {
    fn script(&self, screens: impl IntoIterator<Item = Vec<RawNode>>) {
        *self.trees.lock().unwrap() = screens.into_iter().collect();
    }

    fn script_windows(&self, windows: impl IntoIterator<Item = Vec<WindowInfo>>) {
        *self.windows.lock().unwrap() = windows.into_iter().collect();
    }
}

/// The next scripted item; the last one stays.
fn next<T: Clone + Default>(script: &Mutex<VecDeque<T>>) -> T {
    let mut script = script.lock().unwrap();
    if script.len() > 1 {
        script.pop_front().unwrap()
    } else {
        script.front().cloned().unwrap_or_default()
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
            roots: next(&self.trees),
            source: TreeSource::Helper,
            windows: next(&self.windows),
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
        manufacturer: None,
    }
}

/// The app's window alone on a 1000 × 2000 screen.
fn uncovered() -> Vec<WindowInfo> {
    vec![WindowInfo {
        kind: WindowKind::Application,
        active: true,
        focused: true,
        title: None,
        package: Some("com.example".into()),
        bounds: Rect::new(0, 0, 1000, 2000),
    }]
}

/// The app's window under a system window (a bar, the keyboard) at `bounds`.
fn under(kind: WindowKind, bounds: Rect) -> Vec<WindowInfo> {
    let mut windows = vec![WindowInfo {
        kind,
        active: false,
        focused: false,
        title: None,
        package: Some("com.android.systemui".into()),
        bounds,
    }];
    windows.extend(uncovered());
    windows
}

/// A settings screen with a Wi-Fi switch row whose state is `on`; `shift` moves it (animation).
/// As in Settings, the row takes the tap and the switch only shows the state.
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

/// What the app draws under the status bar is on screen, obscured, and can't be touched there.
#[tokio::test]
async fn a_target_under_the_status_bar_fails_without_input() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([screen(false, 0)]);
    driver.script_windows([under(WindowKind::System, Rect::new(0, 0, 1000, 300))]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let mut timings = Timings::default();
    let observed = session.observe(false, &mut timings).await.unwrap();
    assert!(
        observed
            .text
            .contains(r#"[e1] switch "Wi-Fi" off obscured"#),
        "{}",
        observed.text
    );
    let err = session
        .act(
            Action::Tap {
                target: Target::parse("Wi-Fi").unwrap(),
            },
            &mut timings,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::TargetObscured { .. }), "{err}");
    assert!(driver.inputs.lock().unwrap().is_empty());
}

/// The keyboard slides in over an app that doesn't move for it: the tree is the same in every
/// frame, the windows aren't. Seen on Now in Android's search screen, where a tap came back
/// with the keyboard halfway up.
#[tokio::test]
async fn an_action_settles_once_the_keyboard_has_stopped_moving() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([screen(false, 0)]);
    let keyboard = |top| under(WindowKind::InputMethod, Rect::new(0, top, 1000, top + 800));
    // Before the tap, three frames of the keyboard coming up, then where it stays.
    driver.script_windows([
        uncovered(),
        keyboard(1900),
        keyboard(1500),
        keyboard(1200),
        keyboard(1200),
    ]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let outcome = session
        .act(
            Action::Tap {
                target: Target::parse("Wi-Fi").unwrap(),
            },
            &mut Timings::default(),
        )
        .await
        .unwrap();
    assert!(outcome.settled);
    assert!(outcome.screen.keyboard);
    assert_eq!(
        outcome.screen.obstructions,
        [Rect::new(0, 1200, 1000, 2000)]
    );
}

/// An element under the keyboard is in the tree, but waiting for it is waiting to use it: a
/// flow's next step would otherwise find it covered while the keyboard is still on its way out.
#[tokio::test]
async fn waiting_for_an_element_under_the_keyboard_waits_for_the_keyboard_to_leave() {
    let driver = Arc::new(FakeDriver::default());
    driver.script([screen(false, 0)]);
    let keyboard = under(WindowKind::InputMethod, Rect::new(0, 100, 1000, 2000));
    driver.script_windows([keyboard.clone(), keyboard.clone(), uncovered()]);
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let target = Target::parse("Wi-Fi").unwrap();
    let mut timings = Timings::default();
    let found = session
        .wait(&target, false, Duration::from_secs(5), &mut timings)
        .await
        .unwrap();
    assert!(!found.screen.keyboard, "{}", found.text);

    // While it is covered it has neither appeared nor gone; the timeout says which.
    driver.script_windows([keyboard]);
    let covered = session
        .wait(&target, false, Duration::ZERO, &mut timings)
        .await
        .unwrap_err();
    assert_eq!(
        covered.to_string(),
        "timed out after 0s waiting for Wi-Fi to come out from under the system window that covers it"
    );
    let still_there = session
        .wait(&target, true, Duration::ZERO, &mut timings)
        .await
        .unwrap_err();
    assert_eq!(
        still_there.to_string(),
        "timed out after 0s waiting for Wi-Fi to disappear"
    );
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

/// A list of 100 rows, ten on screen; each swipe moves it eight rows until the last row is shown.
#[derive(Default)]
struct ListDriver {
    swipes: Mutex<i32>,
    /// The windows on screen, when a scenario needs a system bar over the list.
    windows: Vec<WindowInfo>,
}

impl ListDriver {
    const ROWS: i32 = 100;
    const VISIBLE: i32 = 10;
    const PER_SWIPE: i32 = 8;
}

#[async_trait]
impl Driver for ListDriver {
    fn platform(&self) -> Platform {
        Platform::Android
    }
    async fn devices(&self) -> Result<Vec<Device>> {
        Ok(vec![device()])
    }
    async fn ui_tree(&self, _: &Device) -> Result<RawTree> {
        let first =
            (*self.swipes.lock().unwrap() * Self::PER_SWIPE).min(Self::ROWS - Self::VISIBLE);
        let enabled = NodeFlags {
            enabled: true,
            ..NodeFlags::default()
        };
        let rows = (0..Self::VISIBLE)
            .map(|i| RawNode {
                class: "android.widget.TextView".into(),
                text: Some(format!("Row {}", first + i + 1)),
                bounds: Rect::new(0, i * 200, 1000, (i + 1) * 200),
                flags: enabled,
                ..RawNode::default()
            })
            .collect();
        Ok(RawTree {
            roots: vec![RawNode {
                class: "androidx.recyclerview.widget.RecyclerView".into(),
                bounds: Rect::new(0, 0, 1000, 2000),
                flags: NodeFlags {
                    scrollable: true,
                    ..enabled
                },
                children: rows,
                ..RawNode::default()
            }],
            source: TreeSource::Helper,
            windows: self.windows.clone(),
        })
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(Some("com.example/.Messages".into()))
    }
    async fn input(&self, _: &Device, input: &Input) -> Result<()> {
        if matches!(input, Input::Swipe { .. }) {
            *self.swipes.lock().unwrap() += 1;
        }
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

fn scroll_until(goal: &str) -> Action {
    Action::Scroll {
        direction: Direction::Down,
        within: None,
        until: Some(Target::parse(goal).unwrap()),
    }
}

/// How far `scroll --until` gets mustn't depend on how many rows fit on the screen.
#[tokio::test]
async fn scrolling_until_keeps_going_while_the_list_moves_and_stops_at_its_end() {
    let driver = Arc::new(ListDriver::default());
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    let found = session
        .act(scroll_until("Row 95"), &mut Timings::default())
        .await
        .unwrap();
    assert!(
        found.action.contains("found after 11 scrolls"),
        "{}",
        found.action
    );

    let missing = session
        .act(scroll_until("Row 120"), &mut Timings::default())
        .await
        .unwrap_err();
    assert!(
        matches!(missing, Error::ElementNotFound { .. }),
        "{missing}"
    );
    // One more scroll to reach the end, one that no longer moves it.
    assert_eq!(*driver.swipes.lock().unwrap(), 13);
}

/// A row scrolling in under the navigation bar is in the tree before any of it can be tapped:
/// `scroll --until` goes on until it can be, and says so when the list ends with the row there.
#[tokio::test]
async fn scrolling_until_goes_on_while_the_row_is_under_a_system_bar() {
    let driver = Arc::new(ListDriver {
        windows: under(WindowKind::System, Rect::new(0, 1800, 1000, 2000)),
        ..ListDriver::default()
    });
    let mut session = Session::open(Control::new(driver.clone(), device()), None);
    // Row 10 starts in the last place on screen, under the bar.
    let found = session
        .act(scroll_until("Row 10"), &mut Timings::default())
        .await
        .unwrap();
    assert!(
        found.action.contains("found after 1 scrolls"),
        "{}",
        found.action
    );

    // The list ends with Row 100 in that place.
    let covered = session
        .act(scroll_until("Row 100"), &mut Timings::default())
        .await
        .unwrap_err();
    assert!(matches!(covered, Error::TargetObscured { .. }), "{covered}");
}
