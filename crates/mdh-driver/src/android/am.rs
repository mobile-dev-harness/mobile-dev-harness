//! Parsers for activity- and window-manager output. Pure functions over captured text.

use mdh_core::{Error, LaunchInfo, Result};

/// Parses `am start -W` stdout. Errors (`Error: ...` lines) become `LaunchFailed`.
pub fn parse_am_start(target: &str, out: &str) -> Result<LaunchInfo> {
    let mut info = LaunchInfo {
        activity: None,
        total_time_ms: 0,
        reused_existing: false,
        state: None,
    };
    for line in out.lines().map(str::trim) {
        if let Some(message) = line.strip_prefix("Error: ") {
            return Err(Error::LaunchFailed {
                target: target.to_owned(),
                message: message.to_owned(),
            });
        } else if let Some(activity) = line.strip_prefix("Activity: ") {
            info.activity = Some(activity.to_owned());
        } else if let Some(state) = line.strip_prefix("LaunchState: ") {
            info.state = Some(state.split_whitespace().next().unwrap_or(state).to_owned());
        } else if let Some(ms) = line.strip_prefix("TotalTime: ") {
            info.total_time_ms = ms.parse().unwrap_or_default();
        } else if line.starts_with("Warning: Activity not started") {
            info.reused_existing = true;
        }
    }
    Ok(info)
}

/// Parses `cmd package resolve-activity --brief`: the component is the last line, or
/// `No activity found` when the package has no matching activity.
pub fn parse_resolved_activity(out: &str) -> Option<String> {
    let last = out.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    last.contains('/').then(|| last.to_owned())
}

/// Whether `am stack list` lists a task of `package`, as in
/// `  taskId=3084: com.example/com.example.MainActivity bounds=[0,0][1344,2992] userId=0 visible=false`.
/// A task is named after its root activity, whatever is on top of it and while it is closing.
pub fn has_task(stack_list: &str, package: &str) -> bool {
    stack_list
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("taskId=")?.split_once(": "))
        .any(|(_, task)| {
            task.strip_prefix(package)
                .is_some_and(|rest| rest.starts_with('/'))
        })
}

/// Extracts the component from `mFocusedApp=ActivityRecord{98136054 u0 com.android.settings/.Settings t20}`.
pub fn parse_focused_app(dumpsys: &str) -> Option<String> {
    let line = dumpsys.lines().find(|l| l.contains("mFocusedApp="))?;
    line.split_whitespace()
        .find(|t| t.contains('/') && !t.contains('{'))
        .map(str::to_owned)
}

/// `wm density`: `Physical density: 420`, plus `Override density: 400` when changed in settings
/// (the override is what apps lay out with).
pub fn parse_density(out: &str) -> Option<u32> {
    let value = |prefix: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(prefix))
            .and_then(|v| v.trim().parse().ok())
    };
    value("Override density:").or_else(|| value("Physical density:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/android/am/{name}.txt",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn tasks_of_a_package() {
        const APP: &str = "com.google.samples.apps.nowinandroid.demo.debug";
        // The app under a permission dialog, then the same task right after `pm clear`: every
        // activity in it is finishing and the task is still there.
        for name in ["stack_list_api36", "stack_list_cleared_api36"] {
            let out = fixture(name);
            assert!(has_task(&out, APP), "{name}");
            assert!(has_task(&out, "com.google.android.apps.nexuslauncher"));
            // The dialog's app has no task of its own, and a package is not a prefix.
            assert!(!has_task(&out, "com.google.android.permissioncontroller"));
            assert!(!has_task(&out, "com.google.samples.apps.nowinandroid.demo"));
        }
        assert!(!has_task("", APP));
    }

    #[test]
    fn parses_density() {
        assert_eq!(parse_density("Physical density: 420\n"), Some(420));
        assert_eq!(
            parse_density("Physical density: 420\nOverride density: 480\n"),
            Some(480)
        );
        assert_eq!(parse_density("nonsense"), None);
    }

    // Captured from an API 36 emulator.
    const COLD: &str = "Starting: Intent { cmp=com.android.settings/.Settings }
Status: ok
LaunchState: COLD
Activity: com.android.settings/.homepage.SettingsHomepageActivity
TotalTime: 378
WaitTime: 424
Complete
";
    const REUSED: &str = "Starting: Intent { cmp=com.android.settings/.Settings }
Warning: Activity not started, intent has been delivered to currently running top-most instance.
Status: ok
LaunchState: UNKNOWN (0)
Activity: com.android.settings/.homepage.SettingsHomepageActivity
TotalTime: 0
WaitTime: 1
Complete
";
    const BROUGHT_TO_FRONT: &str =
        "Warning: Activity not started, its current task has been brought to the front
Status: ok
Activity: com.android.settings/.Settings$DisplaySettingsActivity
";
    const MISSING: &str = "Starting: Intent { cmp=com.nope/.Main }
Error type 3
Error: Activity class {com.nope/com.nope.Main} does not exist.
";

    #[test]
    fn am_start_cold_launch() {
        let info = parse_am_start("com.android.settings", COLD).unwrap();
        assert_eq!(
            info.activity.as_deref(),
            Some("com.android.settings/.homepage.SettingsHomepageActivity")
        );
        assert_eq!(info.total_time_ms, 378);
        assert!(!info.reused_existing);
    }

    #[test]
    fn am_start_detects_both_reuse_warnings() {
        assert!(parse_am_start("x", REUSED).unwrap().reused_existing);
        assert!(
            parse_am_start("x", BROUGHT_TO_FRONT)
                .unwrap()
                .reused_existing
        );
    }

    #[test]
    fn am_start_error() {
        let err = parse_am_start("com.nope/.Main", MISSING).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn resolve_activity() {
        let out = "priority=0 preferredOrder=0 match=0x108000 specificIndex=-1 isDefault=true\ncom.android.settings/.Settings\n";
        assert_eq!(
            parse_resolved_activity(out).as_deref(),
            Some("com.android.settings/.Settings")
        );
        assert_eq!(parse_resolved_activity("No activity found\n"), None);
    }

    #[test]
    fn focused_app() {
        let out = "  mFocusedApp=ActivityRecord{98136054 u0 com.android.settings/.Settings t20}\n";
        assert_eq!(
            parse_focused_app(out).as_deref(),
            Some("com.android.settings/.Settings")
        );
        assert_eq!(parse_focused_app("  mFocusedApp=null\n"), None);
    }
}
