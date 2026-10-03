//! The MCP surface: tool list and schemas, and a tool call against a scripted driver.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use mdh_control::Control;
use mdh_core::ui::{NodeFlags, RawNode, RawTree, Rect, TreeSource};
use mdh_core::{Device, DeviceState, Error, Input, LaunchInfo, Platform, Result};
use mdh_driver::Driver;
use mdh_mcp::MdhServer;
use rmcp::model::{CallToolRequestParams, ContentBlock};
use rmcp::{ClientHandler, ServiceExt};

/// A single static screen with a "Wi-Fi" button.
struct StaticDriver;

#[async_trait]
impl Driver for StaticDriver {
    fn platform(&self) -> Platform {
        Platform::Android
    }
    async fn devices(&self) -> Result<Vec<Device>> {
        Ok(vec![device()])
    }
    async fn ui_tree(&self, _: &Device) -> Result<RawTree> {
        let button = RawNode {
            class: "android.widget.Button".into(),
            text: Some("Wi-Fi".into()),
            bounds: Rect::new(0, 100, 500, 200),
            flags: NodeFlags {
                enabled: true,
                clickable: true,
                ..NodeFlags::default()
            },
            ..RawNode::default()
        };
        Ok(RawTree {
            roots: vec![RawNode {
                class: "android.widget.FrameLayout".into(),
                bounds: Rect::new(0, 0, 1000, 2000),
                children: vec![button],
                ..RawNode::default()
            }],
            source: TreeSource::Helper,
            windows: Vec::new(),
        })
    }
    async fn foreground_activity(&self, _: &Device) -> Result<Option<String>> {
        Ok(Some("com.example/.Main".into()))
    }
    async fn input(&self, _: &Device, _: &Input) -> Result<()> {
        Ok(())
    }
    async fn screenshot(&self, _: &Device) -> Result<Vec<u8>> {
        Err(Error::NoDevice)
    }
    async fn install(&self, _: &Device, _: &Path, _: bool) -> Result<()> {
        Err(Error::NoDevice)
    }
    async fn launch(&self, _: &Device, _: &str) -> Result<LaunchInfo> {
        Err(Error::NoDevice)
    }
    async fn stop(&self, _: &Device, _: &str) -> Result<()> {
        Err(Error::NoDevice)
    }
}

fn device() -> Device {
    Device {
        id: "fake-1".into(),
        platform: Platform::Android,
        state: DeviceState::Online,
        model: None,
        is_emulator: true,
    }
}

#[derive(Clone, Default)]
struct TestClient;
impl ClientHandler for TestClient {}

async fn connect() -> rmcp::service::RunningService<rmcp::RoleClient, TestClient> {
    let server = MdhServer::with_control(Control::new(Arc::new(StaticDriver), device()));
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        let service = server.serve(server_io).await.expect("server starts");
        service.waiting().await.ok();
    });
    TestClient.serve(client_io).await.expect("client connects")
}

fn text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn lists_the_tools_with_object_schemas() {
    let client = connect().await;
    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "mdh_act",
            "mdh_app",
            "mdh_flow",
            "mdh_impact",
            "mdh_logs",
            "mdh_observe",
            "mdh_run",
            "mdh_status",
            "mdh_verify",
            "mdh_visual",
            "mdh_wait"
        ]
    );
    for tool in &tools {
        assert_eq!(
            tool.input_schema.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "{} needs an object schema",
            tool.name
        );
    }
}

#[tokio::test]
async fn observe_then_act_and_errors_carry_codes() {
    let client = connect().await;
    let call = |name: &'static str, args: serde_json::Value| {
        let params =
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone());
        client.call_tool(params)
    };

    let observed = call("mdh_observe", serde_json::json!({})).await.unwrap();
    assert_eq!(observed.is_error, Some(false));
    assert!(text(&observed.content).contains(r#"[e1] button "Wi-Fi""#));

    let acted = call(
        "mdh_act",
        serde_json::json!({ "actions": [{ "action": "tap", "target": "e1" }] }),
    )
    .await
    .unwrap();
    assert!(text(&acted.content).starts_with(r#"tap e1 button "Wi-Fi" → ok"#));

    let missing = call(
        "mdh_act",
        serde_json::json!({ "actions": [{ "action": "tap", "target": "Bluetooth" }] }),
    )
    .await
    .unwrap();
    assert_eq!(missing.is_error, Some(true));
    assert!(text(&missing.content).contains("error[ELEMENT_NOT_FOUND]"));
}
