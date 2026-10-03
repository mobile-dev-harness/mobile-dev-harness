//! Risks from typical changes to a small Android project in a temporary git repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use mdh_risk::{Dimension, Likelihood, Risk};

const FILES: &[(&str, &str)] = &[
    ("settings.gradle.kts", "include(\":app\")\n"),
    (
        "app/build.gradle.kts",
        "plugins { id(\"com.android.application\") }\nandroid {\n    compileSdk = 35\n    defaultConfig {\n        minSdk = 26\n        targetSdk = 34\n    }\n}\n",
    ),
    (
        "app/src/main/AndroidManifest.xml",
        r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.CAMERA" />
    <application android:label="x">
        <activity android:name=".MainActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
        <activity android:name=".SettingsActivity" android:exported="true" android:screenOrientation="portrait">
            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <data android:scheme="shop" android:host="settings" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/MainActivity.kt",
        r#"package dev.shop

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        findViewById<Button>(R.id.open_settings).setOnClickListener {
            startActivity(Intent(this, SettingsActivity::class.java))
        }
    }
}
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/SettingsActivity.kt",
        r#"package dev.shop

class SettingsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)
        Sync.schedule(this)
    }

    fun askForNotifications() {
        request()
    }
}
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/Sync.kt",
        "package dev.shop\n\nobject Sync {\n    fun schedule(context: Context) {\n        log(context)\n    }\n}\n",
    ),
    (
        "app/src/main/res/layout/activity_main.xml",
        "<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\">\n    <Button android:id=\"@+id/open_settings\" android:text=\"Settings\" />\n</LinearLayout>\n",
    ),
    (
        "app/src/main/res/layout/activity_settings.xml",
        "<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\">\n    <Switch android:id=\"@+id/sync\" android:text=\"Sync\" />\n</LinearLayout>\n",
    ),
];

struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("mdh-compat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, content) in FILES {
            write(&dir, path, content);
        }
        let repo = Repo(dir);
        repo.git(&["init", "-q"]);
        repo.git(&["add", "."]);
        repo.git(&["commit", "-q", "-m", "base"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&self.0)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn edit(&self, path: &str, from: &str, to: &str) {
        let p = self.0.join(path);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains(from), "{path} has no {from:?}");
        std::fs::write(p, text.replacen(from, to, 1)).unwrap();
    }

    fn risks(&self) -> Vec<Risk> {
        let report = mdh_impact::analyze(&mdh_impact::Options {
            project: self.0.clone(),
            base: "HEAD".into(),
        })
        .unwrap();
        mdh_risk::risks(&report)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(root: &Path, path: &str, content: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn find<'r>(risks: &'r [Risk], id: &str) -> &'r Risk {
    risks.iter().find(|r| r.id == id).unwrap_or_else(|| {
        panic!(
            "no {id} in {:?}",
            risks.iter().map(|r| &r.id).collect::<Vec<_>>()
        )
    })
}

#[test]
fn an_api_level_branch_needs_both_sides() {
    let repo = Repo::new("gate");
    repo.edit(
        "app/src/main/kotlin/dev/shop/SettingsActivity.kt",
        "        request()",
        "        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {\n            requestPermissions(arrayOf(POST_NOTIFICATIONS), 1)\n        }",
    );
    let risks = repo.risks();
    let gate = find(&risks, "api-gate:33");
    assert_eq!(gate.dimension, Dimension::Os);
    assert_eq!(gate.likelihood, Likelihood::High);
    let needs: Vec<String> = gate.needs.iter().map(|n| n.describe()).collect();
    assert_eq!(needs, ["API 26–32", "API 33+"]);
    assert_eq!(gate.screens, ["SettingsActivity"]);
    assert!(
        gate.evidence[0].contains("SettingsActivity.askForNotifications"),
        "{:?}",
        gate.evidence
    );
    // targetSdk 34: the notification permission applies.
    let notifications = find(&risks, "behavior:notification-permission");
    assert!(
        notifications.evidence[0].contains("POST_NOTIFICATIONS"),
        "{:?}",
        notifications.evidence
    );
    // One-time permissions apply on API 30+ whatever the target.
    find(&risks, "behavior:one-time-permissions");
    // Nothing drawn changed: no screen-size risk.
    assert!(risks.iter().all(|r| r.id != "screen-size"));
}

#[test]
fn layouts_resources_and_manifest_attributes() {
    let repo = Repo::new("layout");
    repo.edit(
        "app/src/main/res/layout/activity_settings.xml",
        "android:text=\"Sync\"",
        "android:text=\"Sync over mobile data\"",
    );
    write(
        &repo.0,
        "app/src/main/res/layout-sw600dp/activity_settings.xml",
        "<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\" android:orientation=\"horizontal\">\n    <Switch android:id=\"@+id/sync\" />\n</LinearLayout>\n",
    );
    repo.edit(
        "app/src/main/AndroidManifest.xml",
        "android:screenOrientation=\"portrait\"",
        "android:screenOrientation=\"sensorPortrait\"",
    );
    let risks = repo.risks();
    let size = find(&risks, "screen-size");
    assert_eq!(size.dimension, Dimension::ScreenSize);
    let shapes: Vec<String> = size.needs.iter().map(|n| n.describe()).collect();
    assert_eq!(
        shapes,
        ["compact 360×640 dp", "landscape", "tablet 1280×800 dp"]
    );
    let large = find(&risks, "device:large-screen-resources");
    assert_eq!(
        large.evidence,
        [
            "@layout/activity_settings (app/src/main/res/layout-sw600dp/activity_settings.xml:1): sw600dp qualifier"
        ]
    );
    let orientation = find(&risks, "device:orientation-and-resizability");
    assert!(orientation.state_check);
    assert!(
        orientation.evidence[0].contains("android:screenOrientation"),
        "{:?}",
        orientation.evidence
    );
}

#[test]
fn background_work_is_a_vendor_risk_and_target_sdk_brings_its_changes() {
    let repo = Repo::new("vendor");
    repo.edit(
        "app/src/main/kotlin/dev/shop/Sync.kt",
        "        log(context)",
        "        WorkManager.getInstance(context).enqueueUniquePeriodicWork(\"sync\", KEEP, request)",
    );
    repo.edit("app/build.gradle.kts", "targetSdk = 34", "targetSdk = 35");
    let risks = repo.risks();
    let vendor = find(&risks, "vendor:background-restrictions");
    assert_eq!(vendor.dimension, Dimension::Vendor);
    assert!(vendor.needs[0].vendors.contains(&"xiaomi".to_owned()));
    // targetSdk 34 → 35: edge-to-edge, and the app calls setContentView.
    let edge = find(&risks, "behavior:edge-to-edge");
    assert_eq!(edge.likelihood, Likelihood::High);
    assert!(
        edge.evidence[0].contains("targetSdk 34 → 35"),
        "{:?}",
        edge.evidence
    );
    assert_eq!(edge.needs[0].describe(), "API 35+");
}
