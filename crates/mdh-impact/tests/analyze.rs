//! End to end: a small Android project in a temporary git repository, changed in typical ways.

use std::path::{Path, PathBuf};
use std::process::Command;

use mdh_impact::{ChangeKind, ImpactReport, Options};

const FILES: &[(&str, &str)] = &[
    ("settings.gradle.kts", "include(\":app\")\n"),
    (
        "app/build.gradle.kts",
        "plugins { id(\"com.android.application\") }\n",
    ),
    (
        "app/src/main/AndroidManifest.xml",
        r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application android:label="x">
        <activity android:name=".MainActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
        <activity android:name=".LoginActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <data android:scheme="shop" android:host="login" />
            </intent-filter>
        </activity>
        <activity android:name=".AboutActivity" />
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
        findViewById<Button>(R.id.open_login).setOnClickListener {
            startActivity(Intent(this, LoginActivity::class.java))
        }
    }
}
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/LoginActivity.kt",
        r#"package dev.shop

class LoginActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_login)
        title = Greeter.greet("alice")
        Legacy.compute(1)
    }
}
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/AboutActivity.kt",
        r#"package dev.shop

class AboutActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        title = getString(R.string.about)
    }
}
"#,
    ),
    (
        "app/src/main/kotlin/dev/shop/Greeter.kt",
        "package dev.shop\n\nobject Greeter {\n    fun greet(name: String): String = \"Hi $name\"\n}\n",
    ),
    (
        "app/src/main/java/dev/shop/Legacy.java",
        "package dev.shop;\n\npublic class Legacy {\n    public static int compute(int x) {\n        return x + 1;\n    }\n}\n",
    ),
    (
        "app/src/main/res/layout/activity_main.xml",
        r#"<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android">
    <Button android:id="@+id/open_login" android:text="@string/log_in" />
</LinearLayout>
"#,
    ),
    (
        "app/src/main/res/layout/activity_login.xml",
        r#"<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android">
    <EditText android:id="@+id/email" />
</LinearLayout>
"#,
    ),
    (
        "app/src/main/res/values/strings.xml",
        "<resources>\n    <string name=\"log_in\">Log in</string>\n    <string name=\"about\">About</string>\n</resources>\n",
    ),
    (
        "app/src/main/res/values-zh/strings.xml",
        "<resources>\n    <string name=\"log_in\">登录</string>\n</resources>\n",
    ),
    (
        "app/src/test/kotlin/dev/shop/GreeterTest.kt",
        "package dev.shop\n\nclass GreeterTest {\n    fun greets() { Greeter.greet(\"bob\") }\n}\n",
    ),
];

struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("mdh-impact-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, content) in FILES {
            write(&dir, path, content);
        }
        let repo = Repo(dir);
        repo.git(&["init", "-q"]);
        repo.git(&["add", "-A"]);
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
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    fn edit(&self, path: &str, old: &str, new: &str) {
        let p = self.0.join(path);
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains(old), "{path} has no {old:?}");
        std::fs::write(p, s.replacen(old, new, 1)).unwrap();
    }

    fn analyze(&self) -> ImpactReport {
        mdh_impact::analyze(&Options {
            project: self.0.join("app"),
            base: "HEAD".into(),
        })
        .expect("analysis succeeds")
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

#[test]
fn no_changes() {
    let repo = Repo::new("clean");
    let r = repo.analyze();
    assert_eq!(r.files_changed, 0);
    assert!(mdh_impact::render(&r).starts_with("no changes against HEAD"));
}

#[test]
fn a_typical_change() {
    let repo = Repo::new("typical");
    repo.edit(
        "app/src/main/kotlin/dev/shop/Greeter.kt",
        "fun greet(name: String)",
        "fun greet(name: String, formal: Boolean)",
    );
    repo.edit("app/src/main/java/dev/shop/Legacy.java", "x + 1", "x + 2");
    repo.edit(
        "app/src/main/res/values/strings.xml",
        ">Log in<",
        ">Sign in<",
    );
    repo.edit(
        "app/src/main/kotlin/dev/shop/MainActivity.kt",
        "package dev.shop",
        "package dev.shop\n// Entry point.",
    );
    repo.edit("app/build.gradle.kts", "plugins", "// signing\nplugins");

    let r = repo.analyze();
    let changes: Vec<(&str, ChangeKind)> = r
        .changes
        .iter()
        .map(|c| (c.decl.as_str(), c.change))
        .collect();
    assert_eq!(
        changes,
        [
            ("Legacy.compute", ChangeKind::Body),
            ("Greeter.greet", ChangeKind::Signature),
            ("@string/log_in", ChangeKind::Body),
        ]
    );
    assert_eq!(r.cosmetic, ["app/src/main/kotlin/dev/shop/MainActivity.kt"]);
    assert_eq!(r.other_files.len(), 1);
    assert_eq!(r.other_files[0].path, "app/build.gradle.kts");

    let screens: Vec<&str> = r.screens.iter().map(|s| s.screen.as_str()).collect();
    assert_eq!(screens, ["LoginActivity", "MainActivity"]);
    let login = &r.screens[0];
    assert_eq!(
        login.reach,
        ["shop://login", "MainActivity ▸ \"Sign in\" ▸ LoginActivity"]
    );
    assert_eq!(login.changes, 2);
    assert!(!screens.contains(&"AboutActivity"));

    assert_eq!(r.callers.len(), 1);
    let site = &r.callers[0].sites[0];
    assert_eq!(site.note.as_deref(), Some("1 argument, needs 2"));
    assert!(site.file.ends_with("GreeterTest.kt") || site.file.ends_with("LoginActivity.kt"));
    assert_eq!(r.callers[0].sites.len(), 2);

    assert!(
        r.verify
            .compatibility
            .iter()
            .any(|c| c == "@string/log_in changed; translations may be stale: values-zh"),
        "{:?}",
        r.verify.compatibility
    );
    assert_eq!(r.verify.tests, ["GreeterTest"]);
    assert!(
        r.verify
            .ui
            .iter()
            .any(|u| u == "@string/log_in → MainActivity"),
        "{:?}",
        r.verify.ui
    );

    let text = mdh_impact::render(&r).replace(&r.base_commit, "<commit>");
    insta::assert_snapshot!(text);
}

#[test]
fn removals_leave_dangling_uses() {
    let repo = Repo::new("removal");
    repo.edit(
        "app/src/main/res/layout/activity_login.xml",
        "    <EditText android:id=\"@+id/email\" />\n",
        "",
    );
    repo.edit(
        "app/src/main/kotlin/dev/shop/LoginActivity.kt",
        "Legacy.compute(1)",
        "Legacy.compute(1)\n        findViewById<EditText>(R.id.email)",
    );
    let r = repo.analyze();
    let removed: Vec<&str> = r
        .changes
        .iter()
        .filter(|c| c.change == ChangeKind::Removed)
        .map(|c| c.decl.as_str())
        .collect();
    assert_eq!(removed, ["@id/email"]);
    assert_eq!(r.dangling.len(), 1);
    assert_eq!(r.dangling[0].sites[0].from, "LoginActivity.onCreate");
}

#[test]
fn unknown_base_is_an_error() {
    let repo = Repo::new("base");
    let e = mdh_impact::analyze(&Options {
        project: repo.0.clone(),
        base: "no-such-branch".into(),
    })
    .unwrap_err();
    assert_eq!(e.code().as_str(), "UNKNOWN_REVISION");
}
