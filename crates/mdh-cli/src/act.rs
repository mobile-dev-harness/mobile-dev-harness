//! Coordinate-level input and app lifecycle commands.

use std::path::Path;

use mdh_core::{Input, LaunchInfo, Result};
use mdh_driver::Driver;
use serde::Serialize;

use crate::context::Context;
use crate::output::Human;

/// Data of commands that only report success.
#[derive(Serialize)]
pub struct Done {
    pub done: String,
}

impl Human for Done {
    fn human(&self) -> String {
        self.done.clone()
    }
}

pub async fn input(cx: &Context, input: Input, describe: String) -> Result<Done> {
    cx.driver.input(&cx.device, &input).await?;
    Ok(Done { done: describe })
}

impl Human for LaunchInfo {
    fn human(&self) -> String {
        let mut s = format!(
            "launched {} ({} ms)",
            self.activity.as_deref().unwrap_or("?"),
            self.total_time_ms
        );
        if self.reused_existing {
            s.push_str("\nwarning: an existing instance was brought to front; the app may not be on its start screen");
        }
        s
    }
}

pub async fn launch(cx: &Context, app: &str) -> Result<LaunchInfo> {
    cx.driver.launch(&cx.device, app).await
}

pub async fn stop(cx: &Context, package: &str) -> Result<Done> {
    cx.driver.stop(&cx.device, package).await?;
    Ok(Done {
        done: format!("stopped {package}"),
    })
}

pub async fn install(cx: &Context, apk: &Path, grant: bool) -> Result<Done> {
    cx.driver.install(&cx.device, apk, grant).await?;
    Ok(Done {
        done: format!("installed {}", apk.display()),
    })
}
