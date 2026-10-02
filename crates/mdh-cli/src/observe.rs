//! `mdh observe` and `mdh screenshot`.

use std::path::PathBuf;
use std::time::Instant;

use mdh_core::Result;
use mdh_core::ui::{ScreenInfo, TreeSource};
use mdh_driver::Driver;
use mdh_ui::{Jpeg, RefTable, UiTree, compress, render, screenshot_jpeg};
use serde::Serialize;

use crate::context::Context;
use crate::output::{Human, Phases, millis};

#[derive(Serialize)]
pub struct Observation {
    pub screen: ScreenInfo,
    pub source: TreeSource,
    /// The compact text form agents read.
    pub text: String,
    pub tree: UiTree,
}

impl Human for Observation {
    fn human(&self) -> String {
        format!("{}\n{}", screen_line(&self.screen), self.text)
    }
}

/// `screen com.example/.LoginActivity  1080x2400  keyboard  overlay:com.android.permissioncontroller`
pub fn screen_line(screen: &ScreenInfo) -> String {
    let mut line = format!(
        "screen {}  {}x{}",
        screen.activity.as_deref().unwrap_or("?"),
        screen.size.width(),
        screen.size.height()
    );
    if screen.keyboard {
        line.push_str("  keyboard");
    }
    if let Some(overlay) = &screen.overlay {
        line.push_str("  overlay:");
        line.push_str(overlay);
    }
    line
}

pub async fn observe(cx: &Context, phases: &mut Phases) -> Result<Observation> {
    let started = Instant::now();
    let (raw, activity) = tokio::join!(
        cx.driver.ui_tree(&cx.device),
        cx.driver.foreground_activity(&cx.device)
    );
    let raw = raw?;
    phases.push(("observe", millis(started)));

    let mut tree = compress(&raw.roots);
    RefTable::default().assign(&mut tree);
    Ok(Observation {
        screen: ScreenInfo::new(activity?, tree.screen, &raw.windows),
        source: raw.source,
        text: render(&tree),
        tree,
    })
}

#[derive(Serialize)]
pub struct Screenshot {
    pub path: PathBuf,
    #[serde(flatten)]
    pub image: Jpeg,
}

impl Human for Screenshot {
    fn human(&self) -> String {
        format!(
            "{} ({}x{}, {} KB)",
            self.path.display(),
            self.image.width,
            self.image.height,
            self.image.bytes.len().div_ceil(1024)
        )
    }
}

pub async fn screenshot(
    cx: &Context,
    output: PathBuf,
    max_edge: u32,
    phases: &mut Phases,
) -> Result<Screenshot> {
    let started = Instant::now();
    let png = cx.driver.screenshot(&cx.device).await?;
    phases.push(("capture", millis(started)));
    let image = screenshot_jpeg(&png, max_edge, 80)?;
    std::fs::write(&output, &image.bytes)?;
    Ok(Screenshot {
        path: output,
        image,
    })
}
