//! A compatibility run across two scripted devices: an API-level branch verified on both sides,
//! the older side broken.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mdh_compat::{CompatOptions, RiskStatus};
use mdh_control::{Control, Session};
use mdh_core::output::Timings;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource};
use mdh_core::{
    Appearance, AppearanceKind, Device, DeviceState, Input, LaunchInfo, PhysicalDisplay, Platform,
    Result,
};
use mdh_driver::Driver;

/// Two emulators: API 36 shows the welcome text, API 32 doesn't (the old branch is broken).
struct Fake {
    appearance: Mutex<Vec<Appearance>>,
    /// API 32 is an AVD to start rather than a running emulator; true once started.
    avd: Option<Mutex<bool>>,
    stopped: Mutex<Vec<String>>,
}

impl Fake {
    fn new(api32_avd: bool) -> Arc<Fake> {
        Arc::new(Fake {
            appearance: Mutex::new(Vec::new()),
            avd: api32_avd.then(|| Mutex::new(false)),
            stopped: Mutex::new(Vec::new()),
        })
    }

    fn api32_running(&self) -> bool {
        self.avd.as_ref().is_none_or(|m| *m.lock().unwrap())
    }
}

fn device(id: &str, api: u32) -> Device {
    Device {
        id: id.into(),
        platform: Platform::Android,
        state: DeviceState::Online,
        model: None,
        is_emulator: true,
        avd: Some(format!("api{api}")),
        api: Some(api),
        manufacturer: Some("google".into()),
    }
}

fn text(t: &str, id: &str, top: i32) -> RawNode {
    RawNode {
        class: "android.widget.TextView".into(),
        resource_id: Some(format!("com.example:id/{id}")),
        text: Some(t.into()),
        bounds: Rect::new(0, top, 1080, top + 150),
        flags: NodeFlags {
            enabled: true,
            ..NodeFlags::default()
        },
        ..RawNode::default()
    }
}

#[async_trait]
impl Driver for Fake {
    fn platform(&self) -> Platform {
        Platform::Android
    }
    async fn devices(&self) -> Result<Vec<Device>> {
        let mut out = vec![device("emulator-5554", 36)];
        if self.api32_running() {
            out.push(device("emulator-5556", 32));
        }
        Ok(out)
    }
    async fn avds(&self) -> Result<Vec<mdh_core::Avd>> {
        Ok(match &self.avd {
            Some(_) if !self.api32_running() => vec![mdh_core::Avd {
                name: "api32".into(),
                api: Some(32),
                running: None,
            }],
            _ => Vec::new(),
        })
    }
    async fn start_emulator(&self, avd: &str, headless: bool) -> Result<Device> {
        assert_eq!((avd, headless), ("api32", true));
        *self.avd.as_ref().unwrap().lock().unwrap() = true;
        Ok(device("emulator-5556", 32))
    }
    async fn stop_emulator(&self, d: &Device) -> Result<()> {
        self.stopped.lock().unwrap().push(d.id.clone());
        Ok(())
    }
    async fn ui_tree(&self, d: &Device) -> Result<RawTree> {
        let mut children = vec![text("Settings", "title", 200)];
        if d.api == Some(36) {
            children.push(text("Welcome", "welcome", 400));
        }
        Ok(RawTree {
            roots: vec![RawNode {
                class: "android.widget.FrameLayout".into(),
                bounds: Rect::new(0, 0, 1080, 2400),
                flags: NodeFlags {
                    enabled: true,
                    ..NodeFlags::default()
                },
                children,
                ..RawNode::default()
            }],
            source: TreeSource::Helper,
            windows: Vec::new(),
        })
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(Some("com.example/.SettingsActivity".into()))
    }
    async fn input(&self, _: &Device, _: &Input) -> Result<()> {
        Ok(())
    }
    async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
        Err(mdh_core::Error::Unsupported {
            operation: "screenshots".into(),
        })
    }
    async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
        Ok(())
    }
    async fn launch(&self, _: &Device, app: &str) -> Result<LaunchInfo> {
        Ok(LaunchInfo {
            activity: Some(format!("{app}/.SettingsActivity")),
            total_time_ms: 300,
            reused_existing: false,
            state: Some("COLD".into()),
        })
    }
    async fn stop(&self, _: &Device, _: &str) -> Result<()> {
        Ok(())
    }
    async fn density(&self, _: &Device) -> Result<u32> {
        Ok(480)
    }
    async fn physical_display(&self, _: &Device) -> Result<PhysicalDisplay> {
        Ok(PhysicalDisplay {
            width: 1080,
            height: 2400,
            density: 480,
        })
    }
    async fn appearance(&self, _: &Device, kind: &AppearanceKind) -> Result<Appearance> {
        Ok(match kind {
            AppearanceKind::Rotation => Appearance::Rotation(None),
            _ => Appearance::Display {
                size: None,
                density: None,
            },
        })
    }
    async fn set_appearance(&self, _: &Device, value: &Appearance) -> Result<()> {
        self.appearance.lock().unwrap().push(value.clone());
        Ok(())
    }
}

const FILES: &[(&str, &str)] = &[
    ("settings.gradle.kts", "include(\":app\")\n"),
    (
        "app/build.gradle.kts",
        "plugins { id(\"com.android.application\") }\nandroid {\n    defaultConfig {\n        minSdk = 26\n        targetSdk = 30\n    }\n}\n",
    ),
    (
        "app/src/main/AndroidManifest.xml",
        r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application>
        <activity android:name=".SettingsActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/SettingsActivity.kt",
        "package dev.shop\n\nclass SettingsActivity : ComponentActivity() {\n    override fun onCreate(savedInstanceState: Bundle?) {\n        super.onCreate(savedInstanceState)\n        greet()\n    }\n\n    fun greet() {\n        show()\n    }\n}\n",
    ),
    (
        ".mdh/flows/welcome.yaml",
        "name: welcome\napp: com.example\nscreens:\n- SettingsActivity\nsteps: []\nassert:\n- visible \"Welcome\"\n",
    ),
];

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

/// A project whose change branches on API 33, in a temporary git repository.
fn project(name: &str) -> PathBuf {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("mdh-compat-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, content) in FILES {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    let file = dir.join("app/src/main/kotlin/dev/shop/SettingsActivity.kt");
    let src = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        src.replace(
            "        show()",
            "        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) show() else legacy()",
        ),
    )
    .unwrap();
    dir
}

fn options(dir: &Path) -> CompatOptions {
    CompatOptions {
        project: dir.to_owned(),
        flows_dir: dir.join(".mdh/flows"),
        runs_dir: dir.join(".mdh/runs"),
        install: false,
        ..CompatOptions::default()
    }
}

#[tokio::test]
async fn an_api_branch_runs_on_both_sides_and_fails_on_the_old_one() {
    let dir = project("online");
    let driver = Fake::new(false);
    let mut session = Session::open(
        Control::new(driver.clone(), device("emulator-5554", 36)),
        None,
    );
    let report = mdh_compat::run(&mut session, &options(&dir), &mut Timings::default())
        .await
        .unwrap();
    let gate = report
        .risks
        .iter()
        .find(|r| r.risk.id == "api-gate:33")
        .unwrap_or_else(|| panic!("{}", report.text));
    assert_eq!(gate.status, RiskStatus::Failed, "{}", report.text);
    assert_eq!(
        gate.notes,
        ["emulator-5556 (api32, API 32): welcome: visible \"Welcome\": not on screen"]
    );
    // Two cells: the reference on API 36, the other side of the boundary on API 32.
    let cells: Vec<String> = report.plan.cells.iter().map(|c| c.describe()).collect();
    assert_eq!(
        cells,
        [
            "emulator-5554 (api36, API 36)",
            "emulator-5556 (api32, API 32)"
        ]
    );
    // No display or rotation was touched for API cells.
    assert!(driver.appearance.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_emulator_is_started_only_with_consent_and_stopped_after() {
    let dir = project("start");
    let driver = Fake::new(true);
    let mut session = Session::open(
        Control::new(driver.clone(), device("emulator-5554", 36)),
        None,
    );
    let refused = mdh_compat::run(&mut session, &options(&dir), &mut Timings::default())
        .await
        .unwrap_err();
    assert_eq!(refused.code().as_str(), "NEEDS_CONSENT");
    assert!(
        refused.to_string().contains("the emulator api32"),
        "{refused}"
    );
    assert!(!driver.api32_running());

    // Without starting it, the old side stays unverified, saying what's missing.
    let no_start = CompatOptions {
        no_start: true,
        ..options(&dir)
    };
    let report = mdh_compat::run(&mut session, &no_start, &mut Timings::default())
        .await
        .unwrap();
    let gate = report
        .risks
        .iter()
        .find(|r| r.risk.id == "api-gate:33")
        .unwrap();
    assert_eq!(gate.status, RiskStatus::Unverified, "{}", report.text);
    assert_eq!(
        gate.notes,
        ["needs the emulator api32 (API 26–32), left out: this run starts no emulators"]
    );

    let consent = CompatOptions {
        consent: true,
        ..options(&dir)
    };
    let report = mdh_compat::run(&mut session, &consent, &mut Timings::default())
        .await
        .unwrap();
    let gate = report
        .risks
        .iter()
        .find(|r| r.risk.id == "api-gate:33")
        .unwrap();
    assert_eq!(gate.status, RiskStatus::Failed, "{}", report.text);
    assert_eq!(*driver.stopped.lock().unwrap(), ["emulator-5556"]);
    let _ = std::fs::remove_dir_all(&dir);
}
