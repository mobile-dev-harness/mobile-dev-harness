//! Startup measurements against scripted launch times: baselines recorded, regressions caught,
//! noise forgiven.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mdh_control::{Control, Session};
use mdh_core::ui::RawTree;
use mdh_core::{Device, DeviceState, Input, LaunchInfo, Platform, Result};
use mdh_driver::Driver;
use mdh_perf::PerfOptions;
use mdh_verify::{Outcome, Status};

/// Launches take the scripted times, in order.
struct FakeDriver {
    launches: Mutex<VecDeque<u64>>,
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
        unreachable!()
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(None)
    }
    async fn input(&self, _: &Device, _: &Input) -> Result<()> {
        Ok(())
    }
    async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
        unreachable!()
    }
    async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
        Ok(())
    }
    async fn launch(&self, _: &Device, app: &str) -> Result<LaunchInfo> {
        let ms = self.launches.lock().unwrap().pop_front().expect("scripted");
        Ok(LaunchInfo {
            activity: Some(format!("{app}/.Main")),
            total_time_ms: ms,
            reused_existing: false,
            state: Some("COLD".into()),
        })
    }
    async fn stop(&self, _: &Device, _: &str) -> Result<()> {
        Ok(())
    }
}

fn device() -> Device {
    Device {
        id: "emulator-5554".into(),
        platform: Platform::Android,
        state: DeviceState::Online,
        model: Some("sdk_gphone64".into()),
        is_emulator: true,
        avd: Some("Pixel 9".into()),
        api: Some(36),
    }
}

/// One cold-start measurement: the first of `times` is the discarded run.
async fn measure(dir: &Path, times: &[u64]) -> mdh_verify::Verdict {
    let driver = Arc::new(FakeDriver {
        launches: Mutex::new(times.iter().copied().collect()),
    });
    let mut session = Session::open(Control::new(driver, device()), None);
    let options = PerfOptions {
        runs: times.len() - 1,
        trace: false,
        runs_dir: None,
        baselines: dir.to_owned(),
        cool_down: Duration::ZERO,
    };
    mdh_perf::startup(&mut session, "com.example", false, &options)
        .await
        .unwrap()
}

fn lines(verdict: &mdh_verify::Verdict) -> Vec<(Outcome, String)> {
    verdict
        .findings
        .iter()
        .map(|f| {
            (
                f.outcome,
                format!("{} — {}", f.check, f.observed.as_deref().unwrap_or("")),
            )
        })
        .collect()
}

#[tokio::test]
async fn records_a_baseline_then_catches_a_regression() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("mdh-perf-startup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // First measurement: the slow first launch is discarded, the rest becomes the baseline.
    let first = measure(&dir, &[2400, 1000, 1010, 990, 1020, 980]).await;
    assert_eq!(first.status, Status::Pass);
    let found = lines(&first);
    assert_eq!(
        found[0].1,
        "perf: cold start — 1,000 ms median (p90 1,016 ms, ±10 ms, 5 runs)"
    );
    assert!(found[1].1.contains("no baseline yet"), "{found:?}");
    let profile = dir.join("Pixel_9-api36-release");
    assert!(profile.join("startup-com.example.json").is_file());

    // Jitter within the noise and the minimum change passes.
    let same = measure(&dir, &[2400, 1030, 1000, 1040, 1020, 1010]).await;
    assert_eq!(same.status, Status::Pass, "{:?}", lines(&same));

    // 300 ms slower fails, and says by how much; the new numbers wait as a candidate.
    let slow = measure(&dir, &[2400, 1310, 1290, 1300, 1320, 1280]).await;
    assert_eq!(slow.status, Status::Fail);
    let found = lines(&slow);
    assert_eq!(found[0].0, Outcome::Fail);
    assert!(
        found[0]
            .1
            .contains("regressed: 1,300 ms vs baseline 1,000 ms (+300 ms, +30%)"),
        "{found:?}"
    );
    // No trace from a device that can't trace: said, not failed on.
    assert!(
        found
            .iter()
            .any(|(o, l)| *o == Outcome::Warn && l.contains("no trace to explain it")),
        "{found:?}"
    );
    assert!(profile.join("startup-com.example.new.json").is_file());

    // A measurement this noisy can't show the same 300 ms.
    let noisy = measure(&dir, &[2400, 900, 1700, 1000, 1500, 1300]).await;
    assert_eq!(noisy.status, Status::Pass, "{:?}", lines(&noisy));
    let _ = std::fs::remove_dir_all(&dir);
}
