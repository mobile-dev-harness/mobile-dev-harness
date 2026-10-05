use mdh_core::ui::{RawNode, Rect};
use mdh_core::{Error, Result};
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

/// Parses `uiautomator dump` XML into top-level nodes (one per window root).
///
/// Trailing junk such as `UI hierchary dumped to: /dev/tty` is ignored, so raw `exec-out` output can be passed in.
pub fn parse_hierarchy(xml: &str) -> Result<Vec<RawNode>> {
    let start = xml
        .find("<hierarchy")
        .ok_or_else(|| parse_error("no <hierarchy> element"))?;
    let end = xml
        .rfind("</hierarchy>")
        .map(|i| i + "</hierarchy>".len())
        .ok_or_else(|| parse_error("unterminated <hierarchy> element"))?;

    let mut reader = Reader::from_str(&xml[start..end]);
    // Stack of open nodes; index 0 is a synthetic container for the window roots.
    let mut stack: Vec<RawNode> = vec![RawNode::default()];
    loop {
        match reader.read_event().map_err(parse_error)? {
            Event::Start(e) if e.name().as_ref() == b"node" => stack.push(node_from(&e)?),
            Event::Empty(e) if e.name().as_ref() == b"node" => {
                let node = node_from(&e)?;
                stack
                    .last_mut()
                    .expect("stack has a root")
                    .children
                    .push(node);
            }
            Event::End(e) if e.name().as_ref() == b"node" => {
                let node = stack.pop().expect("stack has a root");
                stack
                    .last_mut()
                    .ok_or_else(|| parse_error("unbalanced </node>"))?
                    .children
                    .push(node);
            }
            Event::Eof => break,
            _ => {}
        }
    }

    match <[RawNode; 1]>::try_from(stack) {
        Ok([root]) => Ok(root.children),
        Err(_) => Err(parse_error("unclosed <node> element")),
    }
}

fn node_from(e: &BytesStart) -> Result<RawNode> {
    let mut node = RawNode::default();
    for attr in e.attributes() {
        let attr = attr.map_err(parse_error)?;
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(parse_error)?;
        let flag = || value == "true";
        match attr.key.as_ref() {
            b"class" => node.class = value.into_owned(),
            b"package" => node.package = non_empty(&value),
            b"resource-id" => node.resource_id = non_empty(&value),
            b"text" => node.text = non_empty(&value),
            b"content-desc" => node.desc = non_empty(&value),
            b"hint" => node.hint = non_empty(&value),
            b"bounds" => node.bounds = parse_bounds(&value)?,
            b"clickable" => node.flags.clickable = flag(),
            b"long-clickable" => node.flags.long_clickable = flag(),
            b"checkable" => node.flags.checkable = flag(),
            b"checked" => node.flags.checked = flag(),
            b"enabled" => node.flags.enabled = flag(),
            b"focusable" => node.flags.focusable = flag(),
            b"focused" => node.flags.focused = flag(),
            b"scrollable" => node.flags.scrollable = flag(),
            b"selected" => node.flags.selected = flag(),
            b"password" => node.flags.password = flag(),
            _ => {}
        }
    }
    Ok(node)
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_owned())
}

/// Parses `[left,top][right,bottom]`.
fn parse_bounds(s: &str) -> Result<Rect> {
    let nums: Vec<i32> = s
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .filter(|p| !p.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| parse_error(format!("bad bounds {s:?}")))?;
    match nums[..] {
        [l, t, r, b] => Ok(Rect::new(l, t, r, b)),
        _ => Err(parse_error(format!("bad bounds {s:?}"))),
    }
}

fn parse_error(detail: impl ToString) -> Error {
    Error::Parse {
        tool: "uiautomator".into(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_nodes_and_attributes() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0"><node index="0" text="" resource-id="" class="android.widget.FrameLayout" package="com.example" content-desc="" checkable="false" checked="false" clickable="false" enabled="true" focusable="false" focused="false" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[0,0][1080,2400]"><node index="0" text="Sign &amp; in" resource-id="com.example:id/login" class="android.widget.Button" package="com.example" content-desc="" checkable="false" checked="false" clickable="true" enabled="false" focusable="true" focused="false" scrollable="false" long-clickable="false" password="false" selected="false" bounds="[10,20][110,80]" hint="" /></node></hierarchy>UI hierchary dumped to: /dev/tty"#;
        let roots = parse_hierarchy(xml).unwrap();
        assert_eq!(roots.len(), 1);
        let button = &roots[0].children[0];
        assert_eq!(button.text.as_deref(), Some("Sign & in"));
        assert_eq!(button.resource_id.as_deref(), Some("com.example:id/login"));
        assert_eq!(button.bounds, Rect::new(10, 20, 110, 80));
        assert!(button.flags.clickable);
        assert!(!button.flags.enabled);
        assert_eq!(button.hint, None);
    }

    #[test]
    fn parses_real_dumps() {
        for name in [
            "launcher",
            "nia_topic_under_status_bar",
            "settings",
            "settings_display",
            "settings_network",
            "settings_search",
        ] {
            let path = format!(
                "{}/../../fixtures/android/uiautomator/{name}_api36.xml",
                env!("CARGO_MANIFEST_DIR")
            );
            let xml = std::fs::read_to_string(&path).unwrap();
            let roots = parse_hierarchy(&xml).unwrap();
            assert!(!roots.is_empty(), "{name}: no roots");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(
            parse_hierarchy("ERROR: null root node returned by UiTestAutomationBridge.").is_err()
        );
    }
}
