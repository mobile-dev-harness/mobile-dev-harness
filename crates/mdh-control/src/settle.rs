//! Deciding when the UI has settled after an action (architecture §6).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use mdh_core::Result;
use mdh_core::output::Timings;
use mdh_core::ui::TreeSource;
use mdh_observe::{Role, UiTree};

use crate::{Control, Snapshot};

/// Longer than the ~100 ms accessibility event throttle, so `wait_idle` sees the action's events.
const MIN_SETTLE: Duration = Duration::from_millis(150);
const IDLE_QUIET: Duration = Duration::from_millis(200);
const IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(80);
pub(crate) const SETTLE_TIMEOUT: Duration = Duration::from_secs(5);
/// A tree normally takes ~10 ms; this long means the app's main thread isn't answering
/// accessibility queries (observed: the helper blocked ~10 s per tree while the app was frozen).
/// The request is abandoned at this point rather than waited out.
const UNRESPONSIVE: Duration = Duration::from_millis(2000);

pub(crate) struct Settled {
    pub snapshot: Snapshot,
    /// Two identical trees and no new spinner before the timeout.
    pub settled: bool,
    /// Set when reading the UI took this long: the app is likely blocked, possibly heading for
    /// an ANR. Settling stops early instead of waiting on a frozen app.
    pub unresponsive_ms: Option<u64>,
}

/// Waits until two consecutive trees are identical and no progress indicator that wasn't in
/// `before` is showing, then returns the settled snapshot and whether it settled before the
/// timeout. Spinners animate without accessibility events or tree changes, so without the second
/// condition a tap that starts loading would report the spinner instead of the result (observed).
/// With the slow uiautomator fallback a single tree is taken.
pub(crate) async fn settle(
    control: &Control,
    timings: &mut Timings,
    before: Option<&UiTree>,
) -> Result<Settled> {
    let old_spinners: HashSet<u64> = before.map(|t| spinners(t).collect()).unwrap_or_default();
    let loading = |tree: &UiTree| spinners(tree).any(|k| !old_spinners.contains(&k));
    let started = Instant::now();
    tokio::time::sleep(MIN_SETTLE).await;
    let idle = Instant::now();
    control.wait_idle(IDLE_QUIET, IDLE_TIMEOUT).await?;
    timings.record("settle_idle", idle);
    let polling = Instant::now();

    // `None` when the app didn't answer within UNRESPONSIVE.
    let timed_tree = async || {
        tokio::time::timeout(UNRESPONSIVE, control.tree())
            .await
            .ok()
    };
    let mut unresponsive = None;
    let (mut tree, mut windows, source) = match timed_tree().await {
        Some(first) => first?,
        None => {
            unresponsive = Some(UNRESPONSIVE);
            (UiTree::default(), Vec::new(), TreeSource::Helper)
        }
    };
    let mut settled = source == TreeSource::Uiautomator;
    while !settled && unresponsive.is_none() && started.elapsed() < SETTLE_TIMEOUT {
        tokio::time::sleep(POLL).await;
        let Some(next) = timed_tree().await else {
            unresponsive = Some(UNRESPONSIVE);
            break;
        };
        let (next, next_windows, _) = next?;
        settled = next.fingerprint() == tree.fingerprint() && !loading(&next);
        (tree, windows) = (next, next_windows);
    }

    timings.record("settle_poll", polling);
    let activity = control.driver.foreground_activity(&control.device).await?;
    Ok(Settled {
        snapshot: Snapshot::new(tree, &windows, activity, source),
        settled: settled && unresponsive.is_none(),
        unresponsive_ms: unresponsive.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
    })
}

fn spinners(tree: &UiTree) -> impl Iterator<Item = u64> + '_ {
    tree.iter()
        .filter(|n| n.role == Role::Progress)
        .map(|n| n.key)
}
