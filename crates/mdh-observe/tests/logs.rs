//! Log digests on real and synthetic logcat captures.

use mdh_driver::android::parse_logcat;
use mdh_observe::{AppFilter, CrashKind, digest};

fn app(package: &str, pids: &[u32]) -> AppFilter {
    AppFilter {
        packages: vec![package.to_owned()],
        pids: pids.iter().copied().collect(),
    }
}

#[test]
fn java_crash_from_a_real_capture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/android/logcat/am_crash_settings_api36.txt"
    );
    let entries = parse_logcat(&std::fs::read_to_string(path).unwrap());
    let d = digest(&entries, &app("com.android.settings", &[15922]));

    assert_eq!(
        d.crashes.len(),
        1,
        "the death right after the crash is the crash itself"
    );
    let crash = &d.crashes[0];
    assert_eq!(crash.kind, CrashKind::Java);
    assert_eq!(crash.package.as_deref(), Some("com.android.settings"));
    assert_eq!(crash.pid, Some(15922));
    assert_eq!(
        crash.summary,
        "android.app.RemoteServiceException$CrashedByAdbException: shell-induced crash"
    );
    // Framework-only stack: the first frames are shown, the rest folded.
    assert_eq!(
        crash.frames[0],
        "android.app.ActivityThread.throwRemoteServiceException(ActivityThread.java:2377)"
    );
    assert_eq!(crash.frames.len() + crash.folded_frames, 10);
}

// Synthetic, modeled on crash_dump and ActivityManager output; replace with real captures from the
// sample app once it exists.
const NATIVE_AND_ANR: &str = "\
1790939500.000 10123  4321  4321 I ActivityManager: Start proc 4321:com.example.app/u0a123 for top-activity
1790939500.100 10123  4321  4321 W Auth    : token cache miss
1790939500.200 10123  4321  4400 E Network : timeout after 10000 ms
1790939500.250  1000   667   700 E Other   : not our app
1790939501.000 10124  4500  4500 F DEBUG   : *** *** *** *** *** *** *** *** *** *** *** *** *** *** *** ***
1790939501.000 10124  4500  4500 F DEBUG   : pid: 4321, tid: 4321, name: com.example.app  >>> com.example.app <<<
1790939501.000 10124  4500  4500 F DEBUG   : signal 11 (SIGSEGV), code 1 (SEGV_MAPERR), fault addr 0x0
1790939501.000 10124  4500  4500 F DEBUG   : backtrace:
1790939501.000 10124  4500  4500 F DEBUG   :       #00 pc 000000000001a2b4  /data/app/~~x==/com.example.app-y==/lib/arm64/libnative.so (crash+20)
1790939501.000 10124  4500  4500 F DEBUG   :       #01 pc 0000000000355e30  /apex/com.android.art/lib64/libart.so (art_quick_generic_jni_trampoline+144)
1790939501.100  1000   667   701 I ActivityManager: Process com.example.app (pid 4321) has died: fg  TOP
1790939502.000  1000   667   702 E ActivityManager: ANR in com.example.other (com.example.other/.MainActivity)
1790939502.000  1000   667   702 E ActivityManager: PID: 5555
1790939502.000  1000   667   702 E ActivityManager: Reason: Input dispatching timed out (Waited 5001ms for MotionEvent)
1790939503.000  1000   667   703 I ActivityManager: Process com.example.app (pid 6000) has died: cch+5 CEM
";

#[test]
fn native_crash_anr_and_unexplained_death() {
    let entries = parse_logcat(NATIVE_AND_ANR);
    // pid 4321 isn't known up front; it's picked up from the `Start proc` line.
    let d = digest(&entries, &app("com.example.app", &[]));

    assert_eq!((d.warnings, d.errors), (1, 1));
    assert_eq!(
        d.recent,
        [
            "W/Auth: token cache miss",
            "E/Network: timeout after 10000 ms"
        ]
    );

    let kinds: Vec<CrashKind> = d.crashes.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [CrashKind::Native, CrashKind::Anr, CrashKind::Died]);

    let native = &d.crashes[0];
    assert_eq!(native.package.as_deref(), Some("com.example.app"));
    assert_eq!(native.pid, Some(4321));
    assert!(native.summary.starts_with("signal 11 (SIGSEGV)"));
    assert_eq!(native.frames.len(), 1, "only the app's own frame is shown");
    assert_eq!(native.folded_frames, 1);

    let anr = &d.crashes[1];
    assert_eq!(anr.package.as_deref(), Some("com.example.other"));
    assert_eq!(anr.pid, Some(5555));
    assert!(anr.summary.starts_with("Input dispatching timed out"));

    assert_eq!(d.crashes[2].pid, Some(6000));
}

#[test]
fn repeated_lines_are_counted_not_listed() {
    let line = |t: &str, msg: &str| format!("1790939500.{t} 10123  4321  4321 W Conn    : {msg}\n");
    let log = [
        line("100", "callback not found"),
        line("200", "callback not found"),
        line("300", "other"),
        line("400", "callback not found"),
    ]
    .concat();
    let d = digest(&parse_logcat(&log), &app("com.example.app", &[4321]));
    assert_eq!(d.warnings, 4);
    assert_eq!(
        d.recent,
        ["W/Conn: other", "W/Conn: callback not found (×3)"]
    );
}

#[test]
fn native_crash_from_a_real_capture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/android/logcat/sample_native_crash_api36.txt"
    );
    let entries = parse_logcat(&std::fs::read_to_string(path).unwrap());
    let d = digest(&entries, &app("dev.mdh.sample", &[]));
    let crash = d
        .crashes
        .iter()
        .find(|c| c.kind == CrashKind::Native)
        .expect("a native crash");
    assert_eq!(crash.package.as_deref(), Some("dev.mdh.sample"));
    assert!(
        crash.summary.starts_with("signal 11 (SIGSEGV)"),
        "{}",
        crash.summary
    );
    assert_eq!(crash.frames[0], "#00 libc.so (kill+8)");
    assert!(crash.frames.iter().all(|f| !f.contains("BuildId")));
    assert!(crash.of_app);
}

#[test]
fn java_crash_shows_the_apps_own_frames_and_cause() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/android/logcat/sample_java_crash_api36.txt"
    );
    let entries = parse_logcat(&std::fs::read_to_string(path).unwrap());
    let d = digest(&entries, &app("dev.mdh.sample", &[]));
    let crash = &d.crashes[0];
    assert_eq!(
        crash.summary,
        "java.lang.IllegalStateException: Sample crash: could not pay for the cart"
    );
    assert_eq!(
        crash.frames[0],
        "dev.mdh.sample.Checkout.pay(TroublesActivity.kt:61)"
    );
    assert!(
        crash.frames.iter().all(|f| f.starts_with("dev.mdh.sample")),
        "framework frames are folded"
    );
    assert_eq!(
        crash.caused_by,
        ["java.lang.IllegalArgumentException: cart id must not be blank"]
    );
}

#[test]
fn anr_from_a_real_capture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/android/logcat/sample_anr_api36.txt"
    );
    let entries = parse_logcat(&std::fs::read_to_string(path).unwrap());
    let d = digest(&entries, &app("dev.mdh.sample", &[]));
    let anr = d
        .crashes
        .iter()
        .find(|c| c.kind == CrashKind::Anr)
        .expect("an ANR");
    assert_eq!(anr.package.as_deref(), Some("dev.mdh.sample"));
    assert_eq!(anr.pid, Some(20765));
    assert!(
        anr.summary.starts_with("Input dispatching timed out"),
        "{}",
        anr.summary
    );
    assert!(anr.of_app);
}
