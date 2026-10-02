//! Deciding when the UI has settled after an action (architecture §6).

use std::time::{Duration, Instant};

use mdh_core::Result;
use mdh_core::output::Timings;
use mdh_core::ui::{ScreenInfo, TreeSource};

use crate::{Control, Snapshot};

/// Longer than the ~100 ms accessibility event throttle, so `wait_idle` sees the action's events.
const MIN_SETTLE: Duration = Duration::from_millis(150);
const IDLE_QUIET: Duration = Duration::from_millis(200);
const IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(80);
pub(crate) const SETTLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Waits until two consecutive trees are identical, then returns the settled snapshot and whether
/// it settled before the timeout. With the slow uiautomator fallback a single tree is taken.
pub(crate) async fn settle(control: &Control, timings: &mut Timings) -> Result<(Snapshot, bool)> {
    let started = Instant::now();
    tokio::time::sleep(MIN_SETTLE).await;
    let idle = Instant::now();
    control.wait_idle(IDLE_QUIET, IDLE_TIMEOUT).await?;
    timings.record("settle_idle", idle);
    let polling = Instant::now();

    let (mut tree, mut windows, source) = control.tree().await?;
    let mut settled = source == TreeSource::Uiautomator;
    while !settled && started.elapsed() < SETTLE_TIMEOUT {
        tokio::time::sleep(POLL).await;
        let (next, next_windows, _) = control.tree().await?;
        settled = next.fingerprint() == tree.fingerprint();
        (tree, windows) = (next, next_windows);
    }

    timings.record("settle_poll", polling);
    let activity = control.driver.foreground_activity(&control.device).await?;
    Ok((
        Snapshot {
            screen: ScreenInfo::new(activity, tree.screen, &windows),
            tree,
            source,
        },
        settled,
    ))
}
