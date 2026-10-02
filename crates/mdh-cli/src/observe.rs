//! `mdh observe`: the compact UI tree of the current screen.

use std::time::Instant;

use mdh_core::Result;
use mdh_core::ui::TreeSource;
use mdh_driver::Driver;
use mdh_driver::android::{AndroidDriver, AndroidSdk};
use mdh_ui::{RefTable, UiTree, compress, render};
use serde::Serialize;

use crate::output::{Human, Phases, millis};

#[derive(Serialize)]
pub struct Observation {
    pub source: TreeSource,
    /// The compact text form agents read.
    pub text: String,
    pub tree: UiTree,
}

impl Human for Observation {
    fn human(&self) -> String {
        self.text.clone()
    }
}

pub async fn run(device: Option<&str>, phases: &mut Phases) -> Result<Observation> {
    let driver = AndroidDriver::new(&AndroidSdk::locate()?);
    let device = mdh_driver::select_device(driver.devices().await?, device)?;

    let started = Instant::now();
    let raw = driver.ui_tree(&device).await?;
    phases.push(("ui_tree", millis(started)));

    let mut tree = compress(&raw.roots);
    RefTable::default().assign(&mut tree);
    Ok(Observation {
        source: raw.source,
        text: render(&tree),
        tree,
    })
}
