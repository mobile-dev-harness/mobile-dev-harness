//! The four setups: the same agent, model and prompt, with different access to the device.

use std::path::Path;

use serde::Serialize;

use crate::env::Env;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Setup {
    /// The agent alone: code and builds, no device.
    Alone,
    /// The agent with adb and screenshots through its shell.
    Adb,
    /// The agent with mobile-mcp (and adb).
    MobileMcp,
    /// The agent with mdh: its MCP server and Claude Code plugin (and adb).
    Mdh,
}

pub const ALL: [Setup; 4] = [Setup::Alone, Setup::Adb, Setup::MobileMcp, Setup::Mdh];

impl Setup {
    pub fn parse(s: &str) -> Option<Setup> {
        match s.trim() {
            "a" | "alone" => Some(Setup::Alone),
            "b" | "adb" => Some(Setup::Adb),
            "c" | "mobile-mcp" => Some(Setup::MobileMcp),
            "d" | "mdh" => Some(Setup::Mdh),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Setup::Alone => "agent alone",
            Setup::Adb => "agent + adb",
            Setup::MobileMcp => "agent + mobile-mcp",
            Setup::Mdh => "agent + mdh",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Setup::Alone => "alone",
            Setup::Adb => "adb",
            Setup::MobileMcp => "mobile-mcp",
            Setup::Mdh => "mdh",
        }
    }

    /// MCP servers, as a `--mcp-config` document.
    pub fn mcp(self, env: &Env) -> serde_json::Value {
        let servers = match self {
            Setup::MobileMcp => serde_json::json!({
                "mobile-mcp": {"command": "npx", "args": ["-y", "@mobilenext/mobile-mcp@latest"]}
            }),
            Setup::Mdh => serde_json::json!({
                "mdh": {"command": env.mdh, "args": ["mcp"]}
            }),
            Setup::Alone | Setup::Adb => serde_json::json!({}),
        };
        serde_json::json!({ "mcpServers": servers })
    }

    /// `PATH` for the agent: the device tools only where the setup has them.
    pub fn path(self, env: &Env) -> String {
        let system = std::env::var("PATH").unwrap_or_default();
        let platform = env.sdk.join("platform-tools");
        let keep: Vec<&str> = system
            .split(':')
            .filter(|p| Path::new(p) != platform && !p.ends_with("/Android/sdk/emulator"))
            .collect();
        let mut parts: Vec<String> = Vec::new();
        if self != Setup::Alone {
            parts.push(platform.display().to_string());
        }
        if self == Setup::Mdh {
            parts.push(
                env.mdh
                    .parent()
                    .expect("in target/release")
                    .display()
                    .to_string(),
            );
        }
        parts.extend(keep.iter().map(|s| (*s).to_owned()));
        parts.join(":")
    }

    /// What the agent is told about its environment.
    pub fn brief(self, env: &Env) -> String {
        let device = format!(
            "An Android emulator ({}, API {}) is running and is the only device. Leave the device \
             itself alone: don't start, stop or restart the emulator or the adb server, and don't \
             change its settings or input methods; if it stops responding, say so in your answer.",
            env.serial, env.device.api
        );
        match self {
            Setup::Alone => {
                "You have no device or emulator in this setup: don't try adb or the emulator. \
                 Judge from the code and the build."
                    .into()
            }
            Setup::Adb => format!(
                "{device} adb is on PATH: install the APK from build/outputs/apk/debug/, start activities \
                 with `adb shell am start`, drive the UI with `adb shell input tap|text|swipe|keyevent`, \
                 read it with `adb exec-out uiautomator dump /dev/tty`, take screenshots with \
                 `adb exec-out screencap -p > /tmp/screen.png` and look at them with the Read tool, and \
                 read logs with `adb logcat -d`."
            ),
            Setup::MobileMcp => format!(
                "{device} The mobile-mcp tools drive it: list devices, install and launch apps, list the \
                 elements on screen, tap, type, swipe, press buttons and take screenshots. adb is on PATH \
                 too."
            ),
            Setup::Mdh => format!(
                "{device} The mdh tools (mdh_*) build, install and run the app, show the screen as a compact \
                 tree, act on it, read logs and crashes, and verify the app; the mdh verify skill describes \
                 the workflow. adb is on PATH too."
            ),
        }
    }
}
