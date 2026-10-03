//! Explaining slow startups and janky frames with a Perfetto system trace, summarized by
//! `trace_processor_shell` into the few slices that account for the time.
//!
//! The device side ships with Android (9+); the host side is Perfetto's trace processor, a pinned
//! prebuilt fetched only after the user agreed (`mdh perf setup`) and checked against its SHA-256.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use mdh_core::{Error, Result};
use sha2::{Digest, Sha256};

/// The Perfetto release whose trace processor and SQL modules the queries are written against.
pub const VERSION: &str = "v58.2";

/// Prebuilt trace processors of [`VERSION`]: platform, URL and SHA-256 (from Perfetto's own
/// `get.perfetto.dev/trace_processor` manifest).
const PREBUILTS: &[(&str, &str, &str)] = &[
    (
        "macos-aarch64",
        "https://commondatastorage.googleapis.com/perfetto-luci-artifacts/v58.2/mac-arm64/trace_processor_shell",
        "d29864d1ba3b36855527bb1b0ca3aa7f703cdce338b9680bb922c5c151b358fa",
    ),
    (
        "macos-x86_64",
        "https://commondatastorage.googleapis.com/perfetto-luci-artifacts/v58.2/mac-amd64/trace_processor_shell",
        "3927a2767eadd140db3ff4fe0dfbf1bde35c1f56501149cd367f5cee898bef27",
    ),
    (
        "linux-x86_64",
        "https://commondatastorage.googleapis.com/perfetto-luci-artifacts/v58.2/linux-amd64/trace_processor_shell",
        "58042408e6cc861fb1a731c26bb082dc222285561eaa4e12a48a8b2b90dca7b9",
    ),
    (
        "linux-aarch64",
        "https://commondatastorage.googleapis.com/perfetto-luci-artifacts/v58.2/linux-arm64/trace_processor_shell",
        "0e6e0c5452c505c8d46fe472fd196a0d17d963460727e2ce2013b02aa1309555",
    ),
];

/// What the trace records: scheduling, the app's own and the framework's trace sections, and
/// per-frame jank attribution (Android 12+).
pub fn config(package: &str, max_seconds: u32) -> String {
    format!(
        r#"buffers {{ size_kb: 65536 fill_policy: RING_BUFFER }}
data_sources {{ config {{ name: "linux.process_stats" }} }}
data_sources {{ config {{ name: "android.surfaceflinger.frametimeline" }} }}
data_sources {{ config {{ name: "linux.ftrace" ftrace_config {{ ftrace_events: "sched/sched_switch" ftrace_events: "sched/sched_wakeup" ftrace_events: "task/task_newtask" ftrace_events: "task/task_rename" atrace_categories: "am" atrace_categories: "wm" atrace_categories: "gfx" atrace_categories: "view" atrace_categories: "dalvik" atrace_categories: "binder_driver" atrace_categories: "res" atrace_categories: "input" atrace_apps: "{package}" }} }} }}
duration_ms: {}"#,
        max_seconds * 1000
    )
}

fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn cache_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Caches/mdh"));
    }
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".cache")))
        .map(|c| c.join("mdh"))
}

/// Where a downloaded trace processor lives.
pub fn cached_path() -> Option<PathBuf> {
    cache_dir().map(|d| d.join(format!("trace_processor_shell-{VERSION}")))
}

/// The same prebuilt, if Perfetto's own `trace_processor` script fetched it already (it keeps
/// them as `trace_processor_shell-<first 16 hex digits of the SHA-256>`).
fn perfetto_cached() -> Option<PathBuf> {
    let (_, _, sha) = PREBUILTS.iter().find(|(p, _, _)| *p == platform())?;
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(home.join(format!(
        ".local/share/perfetto/prebuilts/trace_processor_shell-{}",
        &sha[..16]
    )))
}

/// `MDH_TRACE_PROCESSOR`, then the downloaded one (ours or Perfetto's), then
/// `trace_processor_shell` on PATH.
pub fn locate() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MDH_TRACE_PROCESSOR")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(p);
    }
    if let Some(p) = [cached_path(), perfetto_cached()]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    {
        return Some(p);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("trace_processor_shell"))
            .find(|p| p.is_file())
    })
}

/// Downloads the pinned trace processor for this platform (about 14 MB), checks its SHA-256 and
/// installs it in the cache. Only call after the user agreed.
pub fn download() -> Result<PathBuf> {
    let platform = platform();
    let Some((_, url, sha)) = PREBUILTS.iter().find(|(p, _, _)| *p == platform) else {
        return Err(Error::ToolNotFound {
            name: "trace_processor_shell".into(),
            hint: format!(
                "Perfetto publishes no prebuilt trace processor for {platform}; build or install it \
                 and put it on PATH, or set MDH_TRACE_PROCESSOR"
            ),
        });
    };
    let target = cached_path().ok_or_else(|| Error::ToolNotFound {
        name: "trace_processor_shell".into(),
        hint: "HOME is not set, so there's no cache to download it to; set MDH_TRACE_PROCESSOR"
            .into(),
    })?;
    std::fs::create_dir_all(target.parent().expect("has a parent"))?;
    let partial = target.with_extension("download");
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "2", "-o"])
        .arg(&partial)
        .arg(url)
        .status()
        .map_err(|_| Error::ToolNotFound {
            name: "curl".into(),
            hint:
                "install curl, or download the trace processor yourself and set MDH_TRACE_PROCESSOR"
                    .into(),
        })?;
    if !status.success() {
        return Err(Error::CommandFailed {
            command: format!("curl {url}"),
            code: status.code(),
            stderr: "download failed".into(),
        });
    }
    let bytes = std::fs::read(&partial)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != *sha {
        let _ = std::fs::remove_file(&partial);
        return Err(Error::Parse {
            tool: "trace processor download".into(),
            detail: format!("SHA-256 {digest} doesn't match the expected {sha}"),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&partial, &target)?;
    Ok(target)
}

/// Runs one SQL query over a trace; rows as header → value maps.
fn query(tp: &Path, trace: &Path, sql: &str) -> Result<Vec<HashMap<String, String>>> {
    let file = trace.with_extension("sql");
    std::fs::write(&file, sql)?;
    let out = Command::new(tp).arg("-q").arg(&file).arg(trace).output()?;
    let _ = std::fs::remove_file(&file);
    if !out.status.success() {
        return Err(Error::CommandFailed {
            command: "trace_processor_shell".into(),
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(parse_csv(&String::from_utf8_lossy(&out.stdout)))
}

/// trace_processor's CSV: a header line, quoted strings, numbers bare.
pub(crate) fn parse_csv(out: &str) -> Vec<HashMap<String, String>> {
    let mut lines = out.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let header = split_csv(header);
    lines
        .map(|l| header.iter().cloned().zip(split_csv(l)).collect())
        .collect()
}

fn split_csv(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// A slice on the app's main thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub id: i64,
    pub parent: Option<i64>,
    pub name: String,
    pub dur_ms: f64,
}

fn slices(rows: &[HashMap<String, String>]) -> Vec<Slice> {
    rows.iter()
        .filter_map(|r| {
            Some(Slice {
                id: r.get("id")?.parse().ok()?,
                parent: r.get("parent_id").and_then(|p| p.parse().ok()),
                name: r.get("name")?.clone(),
                dur_ms: r.get("dur")?.parse::<f64>().ok()? / 1e6,
            })
        })
        .collect()
}

/// `Open dex file /data/app/~~x/dev.app-y/base.apk` → `Open dex file base.apk`; frame numbers
/// and the like go, so repeated work reads (and adds up) as one: `Choreographer#doFrame 64486` →
/// `Choreographer#doFrame`.
fn short_name(name: &str) -> String {
    let name = name.split(" - resynced").next().unwrap_or(name);
    name.split(' ')
        // Numbers and `key=value` details differ between otherwise identical events (paths
        // can contain `=` too, but are shortened below).
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()) && (w.contains('/') || !w.contains('=')))
        .map(|w| {
            if let Some(slash) = w.find('/') {
                // `OpenDexFilesFromOat(/data/app/…/base.apk)` → `OpenDexFilesFromOat(base.apk)`.
                let open = w[..slash].rfind('(').map_or(0, |i| i + 1);
                let tail = w.trim_end_matches(')');
                let closing = &w[tail.len()..];
                let file = tail.rsplit('/').next().unwrap_or(tail);
                format!("{}{file}{closing}", &w[..open])
            } else {
                w.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Slices with the same names on the way down from the root add up: 31 binds in 31 prefetches
/// are one line, `RV Prefetch ×31 760 ms → … → slowBind ×31 744 ms`.
#[derive(Default)]
struct Node {
    name: String,
    count: usize,
    ms: f64,
    children: Vec<usize>,
}

/// The biggest top-level work, each followed down its biggest part while that part accounts for
/// most of it: `bindApplication 687 ms → OpenDexFilesFromOat(base.apk) 600 ms`.
pub fn explain(slices: &[Slice], top: usize) -> Vec<String> {
    let by_id: HashMap<i64, &Slice> = slices.iter().map(|s| (s.id, s)).collect();
    // Each slice's node: the path of names from its root.
    let mut nodes: Vec<Node> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut index: HashMap<(Option<usize>, String), usize> = HashMap::new();
    let mut node_of: HashMap<i64, usize> = HashMap::new();
    fn node(
        s: &Slice,
        by_id: &HashMap<i64, &Slice>,
        nodes: &mut Vec<Node>,
        roots: &mut Vec<usize>,
        index: &mut HashMap<(Option<usize>, String), usize>,
        node_of: &mut HashMap<i64, usize>,
    ) -> usize {
        if let Some(n) = node_of.get(&s.id) {
            return *n;
        }
        let parent = s
            .parent
            .and_then(|p| by_id.get(&p))
            .map(|p| node(p, by_id, nodes, roots, index, node_of));
        let name = short_name(&s.name);
        let n = *index.entry((parent, name.clone())).or_insert_with(|| {
            nodes.push(Node {
                name,
                ..Node::default()
            });
            let n = nodes.len() - 1;
            match parent {
                Some(p) => nodes[p].children.push(n),
                None => roots.push(n),
            }
            n
        });
        nodes[n].count += 1;
        nodes[n].ms += s.dur_ms;
        node_of.insert(s.id, n);
        n
    }
    for s in slices {
        node(s, &by_id, &mut nodes, &mut roots, &mut index, &mut node_of);
    }
    let label = |n: &Node| match n.count {
        1 => format!("{} {:.0} ms", n.name, n.ms),
        c => format!("{} ×{c} {:.0} ms", n.name, n.ms),
    };
    roots.sort_by(|a, b| nodes[*b].ms.total_cmp(&nodes[*a].ms));
    roots
        .into_iter()
        .take(top)
        .map(|root| {
            let mut chain = vec![label(&nodes[root])];
            let mut current = root;
            while let Some(&child) = nodes[current]
                .children
                .iter()
                .max_by(|a, b| nodes[**a].ms.total_cmp(&nodes[**b].ms))
            {
                if nodes[child].ms < nodes[current].ms * 0.5 {
                    break;
                }
                // A frame resynced to a later vsync nests in itself; say it once.
                if nodes[child].name != nodes[current].name {
                    chain.push(label(&nodes[child]));
                }
                current = child;
            }
            // The root and where the time ends up say it; the framework in between doesn't.
            if chain.len() > 4 {
                chain.drain(1..chain.len() - 2);
                chain.insert(1, "…".into());
            }
            chain.join(" → ")
        })
        .collect()
}

fn main_thread_slices(package: &str, window: &str) -> String {
    format!(
        "SELECT s.id, s.parent_id, s.depth, s.name, s.dur
FROM slice s
JOIN thread_track tt ON s.track_id = tt.id
JOIN thread t USING (utid)
JOIN process p USING (upid)
WHERE p.upid IN {} AND t.is_main_thread = 1 AND s.depth <= 24 AND {window}
ORDER BY s.dur DESC LIMIT 5000;",
        app_processes(package)
    )
}

/// The app's processes. Forked from the zygote, an app can stay named `zygote64` in the trace
/// when no rename was recorded; its main thread carries the last 15 characters of the package.
fn app_processes(package: &str) -> String {
    let tail: String = {
        let chars: Vec<char> = package.chars().collect();
        chars[chars.len().saturating_sub(15)..].iter().collect()
    };
    format!(
        "(SELECT upid FROM process WHERE name = '{package}' \
         UNION SELECT upid FROM thread WHERE is_main_thread = 1 AND name = '{tail}')"
    )
}

/// Where a cold start's time went.
pub fn explain_startup(tp: &Path, trace: &Path, package: &str) -> Result<Vec<String>> {
    let startups = query(
        tp,
        trace,
        &format!(
            "INCLUDE PERFETTO MODULE android.startup.startups;
SELECT ts, dur, startup_type FROM android_startups WHERE package = '{package}' ORDER BY ts DESC LIMIT 1;"
        ),
    )?;
    let Some(s) = startups.first() else {
        return Ok(vec!["the trace has no startup of the app".into()]);
    };
    let (ts, dur) = (
        s.get("ts").cloned().unwrap_or_default(),
        s.get("dur").cloned().unwrap_or_default(),
    );
    let window = format!("s.ts >= {ts} AND s.ts < {ts} + {dur}");
    let rows = query(tp, trace, &main_thread_slices(package, &window))?;
    let mut lines = vec![format!(
        "{} start {:.0} ms in the trace; main thread:",
        s.get("startup_type").map_or("", String::as_str),
        dur.parse::<f64>().unwrap_or(0.0) / 1e6
    )];
    lines.extend(
        explain(&slices(&rows), 4)
            .into_iter()
            .map(|l| format!("  {l}")),
    );
    lines.extend(gc(tp, trace, package, &window)?);
    Ok(lines)
}

/// Which frames the app made late, and what its main thread did during them.
pub fn explain_jank(tp: &Path, trace: &Path, package: &str) -> Result<Vec<String>> {
    let frames = query(
        tp,
        trace,
        &format!(
            "SELECT count(*) AS total,
  sum(a.jank_type GLOB '*App Deadline Missed*') AS late,
  max(CASE WHEN a.jank_type GLOB '*App Deadline Missed*' THEN a.dur END) AS worst
FROM actual_frame_timeline_slice a WHERE a.upid IN {};",
            app_processes(package)
        ),
    )?;
    let f = frames.first().cloned().unwrap_or_default();
    let late: u64 = f.get("late").and_then(|v| v.parse().ok()).unwrap_or(0);
    let total: u64 = f.get("total").and_then(|v| v.parse().ok()).unwrap_or(0);
    if total == 0 {
        return Ok(vec![
            "the trace has no frame timeline (needs Android 12+)".into(),
        ]);
    }
    if late == 0 {
        return Ok(vec![format!(
            "{total} frames, none late because of the app"
        )]);
    }
    let worst = f
        .get("worst")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
        / 1e6;
    let window = format!(
        "EXISTS (SELECT 1 FROM actual_frame_timeline_slice a
  WHERE a.upid IN {} AND a.jank_type GLOB '*App Deadline Missed*'
  AND s.ts < a.ts + a.dur AND s.ts + s.dur > a.ts)",
        app_processes(package)
    );
    let rows = query(tp, trace, &main_thread_slices(package, &window))?;
    let mut lines = vec![format!(
        "{late} of {total} frames late because of the app (worst {worst:.0} ms); main thread in those frames:"
    )];
    lines.extend(
        explain(&slices(&rows), 3)
            .into_iter()
            .map(|l| format!("  {l}")),
    );
    lines.extend(gc(tp, trace, package, &window)?);
    Ok(lines)
}

/// Where the app's main thread spent its time over the whole trace: work that costs CPU without
/// making any one frame slow (RecyclerView prefetch between frames, say).
pub fn explain_busy(tp: &Path, trace: &Path, package: &str) -> Result<Vec<String>> {
    let rows = query(tp, trace, &main_thread_slices(package, "1 = 1"))?;
    let slices = slices(&rows);
    if slices.is_empty() {
        return Ok(vec!["the trace has no main-thread work of the app".into()]);
    }
    let mut lines = vec!["main thread, busiest work:".to_owned()];
    lines.extend(explain(&slices, 3).into_iter().map(|l| format!("  {l}")));
    lines.extend(gc(tp, trace, package, "1 = 1")?);
    Ok(lines)
}

fn gc(tp: &Path, trace: &Path, package: &str, window: &str) -> Result<Vec<String>> {
    let rows = query(
        tp,
        trace,
        &format!(
            "SELECT count(*) AS n, sum(s.dur) AS dur FROM slice s
JOIN thread_track tt ON s.track_id = tt.id JOIN thread t USING (utid) JOIN process p USING (upid)
WHERE p.upid IN {} AND s.name GLOB '*GC*' AND s.depth = 0 AND {window};",
            app_processes(package)
        ),
    )?;
    let r = rows.first().cloned().unwrap_or_default();
    let n: u64 = r.get("n").and_then(|v| v.parse().ok()).unwrap_or(0);
    let ms = r
        .get("dur")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
        / 1e6;
    Ok(if n > 0 {
        vec![format!(
            "  garbage collection: {n} time{}, {ms:.0} ms",
            if n == 1 { "" } else { "s" }
        )]
    } else {
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_with_quotes() {
        let rows = parse_csv("\"name\",\"dur\"\n\"a, \"\"b\"\"\",12\n\"c\",3\n");
        assert_eq!(rows[0]["name"], "a, \"b\"");
        assert_eq!(rows[1]["dur"], "3");
    }

    #[test]
    fn explains_the_biggest_chains() {
        let s = |id, parent, name: &str, ms: f64| Slice {
            id,
            parent,
            name: name.into(),
            dur_ms: ms,
        };
        let slices = vec![
            s(1, None, "bindApplication", 687.0),
            s(
                2,
                Some(1),
                "OpenDexFilesFromOat(/data/app/~~x==/dev.app-y==/base.apk)",
                600.0,
            ),
            s(
                3,
                Some(2),
                "Extract dex file /data/app/~~x/dev.app-y/base.apk",
                244.0,
            ),
            s(4, None, "activityStart", 155.0),
            s(5, Some(4), "inflate", 45.0),
            s(6, None, "RV Prefetch", 30.0),
            s(7, Some(6), "RV onBindViewHolder type=0x0", 25.0),
            s(8, None, "RV Prefetch", 30.0),
            s(9, Some(8), "RV onBindViewHolder type=0x0", 25.0),
            s(10, None, "Choreographer#doFrame 64486", 20.0),
            s(
                11,
                Some(10),
                "dispatchInputEvent MotionEvent ACTION_MOVE historySize=2",
                18.0,
            ),
        ];
        assert_eq!(
            explain(&slices, 4),
            [
                "bindApplication 687 ms → OpenDexFilesFromOat(base.apk) 600 ms",
                "activityStart 155 ms",
                "RV Prefetch ×2 60 ms → RV onBindViewHolder ×2 50 ms",
                "Choreographer#doFrame 20 ms → dispatchInputEvent MotionEvent ACTION_MOVE 18 ms",
            ]
        );
    }

    #[test]
    fn every_platform_has_a_checksum() {
        for (_, url, sha) in PREBUILTS {
            assert!(url.contains(VERSION));
            assert_eq!(sha.len(), 64);
        }
    }
}
