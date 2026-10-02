//! Functional checks: assertions on the screen and the logs (functional design F6.1).
//!
//! The inline form (`enabled id=sign_in`) is what agents and the CLI write and what verdicts show;
//! flow files use the same assertions as YAML (`- enabled: id=sign_in`).

use std::fmt;

use mdh_control::{Selector, Target, TextMatch, find_matches, find_one};
use mdh_core::{Error, LogLevel, Result};
use mdh_observe::{Role, UiNode, UiTree, render_line};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::check::{CheckKind, Finding, Outcome};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assertion {
    /// On screen and not covered.
    Visible(Element),
    NotVisible(Element),
    Enabled(Element),
    Disabled(Element),
    Checked(Element),
    Unchecked(Element),
    Focused(Element),
    /// The element's text (an input's value, otherwise its label).
    Text {
        target: Element,
        #[serde(flatten)]
        expect: TextExpect,
    },
    /// The activity in front: `.LoginActivity`, `LoginActivity` or `dev.app/.LoginActivity`.
    Screen(String),
    /// No crash or ANR of the app during the verification window.
    NoCrash,
    /// An app log line contains the text (ignoring case).
    Log {
        contains: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<LogLevel>,
    },
    NoLog {
        contains: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<LogLevel>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextExpect {
    Equals(String),
    /// Ignoring case.
    Contains(String),
}

/// A target in an assertion: written like a CLI target (`id=sign_in`, `"Sign in"`) or, in YAML, as
/// a map (`{ id: sign_in }`, `{ text: Sign in, role: button }`, `{ text_contains: sign }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element(pub Target);

impl Serialize for Element {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Element {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Map {
            id: Option<String>,
            text: Option<String>,
            text_contains: Option<String>,
            label: Option<String>,
            role: Option<String>,
            index: Option<usize>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            Text(String),
            Map(Map),
        }
        match Form::deserialize(d)? {
            Form::Text(s) => Target::parse(&s)
                .map(Element)
                .map_err(serde::de::Error::custom),
            Form::Map(m) => {
                let role =
                    match m.role {
                        Some(r) => Some(Role::parse(&r).ok_or_else(|| {
                            serde::de::Error::custom(format!("unknown role `{r}`"))
                        })?),
                        None => None,
                    };
                let text = match (m.text, m.text_contains, m.label) {
                    (Some(t), None, None) => Some(TextMatch::Exact(t)),
                    (None, Some(t), None) => Some(TextMatch::Contains(t)),
                    (None, None, Some(t)) => Some(TextMatch::Label(t)),
                    (None, None, None) => None,
                    _ => {
                        return Err(serde::de::Error::custom(
                            "use only one of `text`, `text_contains` and `label`",
                        ));
                    }
                };
                if m.id.is_none() && text.is_none() && role.is_none() {
                    return Err(serde::de::Error::custom(
                        "a target needs `id`, `text`, `text_contains`, `label` or `role`",
                    ));
                }
                Ok(Element(Target::Selector(Selector {
                    id: m.id,
                    text,
                    role,
                    index: m.index,
                })))
            }
        }
    }
}

impl fmt::Display for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            // A bare label reads as one: `visible "Sign in"`.
            Target::Selector(Selector {
                id: None,
                text: Some(TextMatch::Label(l)),
                role: None,
                index: None,
            }) => write!(f, "{l:?}"),
            t => t.fmt(f),
        }
    }
}

impl fmt::Display for Assertion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Assertion::Visible(e) => write!(f, "visible {e}"),
            Assertion::NotVisible(e) => write!(f, "not visible {e}"),
            Assertion::Enabled(e) => write!(f, "enabled {e}"),
            Assertion::Disabled(e) => write!(f, "disabled {e}"),
            Assertion::Checked(e) => write!(f, "checked {e}"),
            Assertion::Unchecked(e) => write!(f, "unchecked {e}"),
            Assertion::Focused(e) => write!(f, "focused {e}"),
            Assertion::Text { target, expect } => match expect {
                TextExpect::Equals(v) => write!(f, "text {target} == {v:?}"),
                TextExpect::Contains(v) => write!(f, "text {target} ~= {v:?}"),
            },
            Assertion::Screen(s) => write!(f, "screen {s}"),
            Assertion::NoCrash => f.write_str("no crash"),
            Assertion::Log { contains, .. } => write!(f, "log ~= {contains:?}"),
            Assertion::NoLog { contains, .. } => write!(f, "no log ~= {contains:?}"),
        }
    }
}

impl Assertion {
    /// Parses the inline form:
    ///
    /// ```text
    /// visible TARGET | not visible TARGET | enabled|disabled|checked|unchecked|focused TARGET
    /// text TARGET == VALUE | text TARGET ~= VALUE | screen ACTIVITY | no crash
    /// log ~= TEXT | no log ~= TEXT
    /// ```
    ///
    /// `TARGET` is written like everywhere else: a ref, a selector or a label.
    pub fn parse(s: &str) -> Result<Assertion> {
        let s = s.trim();
        let (head, rest) = split_word(s);
        let head = head.to_lowercase().replace('_', "-");
        let (head, rest) = match head.as_str() {
            "not" | "no" => {
                let (second, rest) = split_word(rest);
                (format!("{head}-{}", second.to_lowercase()), rest)
            }
            _ => (head, rest),
        };
        let element = || -> Result<Element> {
            if rest.is_empty() {
                return Err(invalid(s, "needs a target"));
            }
            Target::parse(unquote(rest)).map(Element)
        };
        let contains = || -> Result<String> {
            let value = rest
                .strip_prefix("~=")
                .ok_or_else(|| invalid(s, "write `log ~= TEXT`"))?;
            Ok(unquote(value.trim()).to_owned())
        };
        Ok(match head.as_str() {
            "visible" => Assertion::Visible(element()?),
            "not-visible" | "gone" => Assertion::NotVisible(element()?),
            "enabled" => Assertion::Enabled(element()?),
            "disabled" => Assertion::Disabled(element()?),
            "checked" => Assertion::Checked(element()?),
            "unchecked" => Assertion::Unchecked(element()?),
            "focused" => Assertion::Focused(element()?),
            "text" => {
                let (target, expect) = match (rest.rfind(" == "), rest.rfind(" ~= ")) {
                    (Some(i), j) if j.is_none_or(|j| i > j) => (
                        &rest[..i],
                        TextExpect::Equals(unquote(rest[i + 4..].trim()).to_owned()),
                    ),
                    (_, Some(j)) => (
                        &rest[..j],
                        TextExpect::Contains(unquote(rest[j + 4..].trim()).to_owned()),
                    ),
                    _ => {
                        return Err(invalid(
                            s,
                            "write `text TARGET == VALUE` or `text TARGET ~= VALUE`",
                        ));
                    }
                };
                Assertion::Text {
                    target: Element(Target::parse(unquote(target.trim()))?),
                    expect,
                }
            }
            "screen" if !rest.is_empty() => Assertion::Screen(rest.to_owned()),
            "no-crash" if rest.is_empty() => Assertion::NoCrash,
            "log" => Assertion::Log {
                contains: contains()?,
                level: None,
            },
            "no-log" => Assertion::NoLog {
                contains: contains()?,
                level: None,
            },
            _ => {
                return Err(invalid(
                    s,
                    "unknown check; use visible, not visible, enabled, disabled, checked, unchecked, \
                     focused, text, screen, no crash, log or no log",
                ));
            }
        })
    }

    /// Checks on the screen, polled until they hold; the rest is evaluated once.
    pub fn is_screen_check(&self) -> bool {
        !matches!(
            self,
            Assertion::NoCrash | Assertion::Log { .. } | Assertion::NoLog { .. }
        )
    }

    /// Evaluates a screen check against the current tree and foreground activity.
    pub fn evaluate(&self, tree: &UiTree, activity: Option<&str>) -> Finding {
        let finding = |outcome: Outcome, observed: Option<String>| Finding {
            kind: CheckKind::Functional,
            outcome,
            check: self.to_string(),
            observed,
            step: None,
            evidence: Vec::new(),
        };
        let pass = || finding(Outcome::Pass, None);
        let fail = |observed: String| finding(Outcome::Fail, Some(observed));
        let state = |e: &Element, ok: &dyn Fn(&UiNode) -> bool, was: &dyn Fn(&UiNode) -> String| {
            match find_one(&e.0, tree) {
                Ok(n) if ok(n) => pass(),
                Ok(n) => fail(format!("{}: {}", was(n), render_line(n))),
                Err(err) => missing(err, &finding),
            }
        };
        match self {
            Assertion::Visible(e) => {
                let matches = find_matches(&e.0, tree);
                if matches.iter().any(|n| !n.state.obscured) {
                    pass()
                } else if let Some(n) = matches.first() {
                    fail(format!(
                        "on screen but covered by system windows: {}",
                        render_line(n)
                    ))
                } else {
                    match find_one(&e.0, tree) {
                        Err(err) => missing(err, &finding),
                        Ok(n) => fail(format!("covered: {}", render_line(n))),
                    }
                }
            }
            Assertion::NotVisible(e) => match find_matches(&e.0, tree).first() {
                None => pass(),
                Some(n) => fail(format!("on screen: {}", render_line(n))),
            },
            Assertion::Enabled(e) => state(e, &|n| !n.state.disabled, &|_| "disabled".into()),
            Assertion::Disabled(e) => state(e, &|n| n.state.disabled, &|_| "enabled".into()),
            Assertion::Checked(e) => state(e, &|n| n.state.checked == Some(true), &|n| match n
                .state
                .checked
            {
                Some(false) => "unchecked".into(),
                _ => "not checkable".into(),
            }),
            Assertion::Unchecked(e) => state(e, &|n| n.state.checked == Some(false), &|n| match n
                .state
                .checked
            {
                Some(true) => "checked".into(),
                _ => "not checkable".into(),
            }),
            Assertion::Focused(e) => state(e, &|n| n.state.focused, &|_| "not focused".into()),
            Assertion::Text { target, expect } => match find_one(&target.0, tree) {
                Ok(n) => {
                    // An empty input has no value; its label is only the hint.
                    let text = if n.role == Role::Textbox {
                        n.value.as_deref().unwrap_or("")
                    } else {
                        n.value.as_deref().or(n.label.as_deref()).unwrap_or("")
                    };
                    let ok = match expect {
                        TextExpect::Equals(v) => text == v,
                        TextExpect::Contains(v) => [&n.label, &n.value, &n.detail]
                            .into_iter()
                            .flatten()
                            .any(|t| t.to_lowercase().contains(&v.to_lowercase())),
                    };
                    if ok {
                        pass()
                    } else {
                        fail(format!("{text:?}: {}", render_line(n)))
                    }
                }
                Err(err) => missing(err, &finding),
            },
            Assertion::Screen(expected) => {
                let shown = activity.unwrap_or("unknown");
                if activity.is_some_and(|a| same_screen(a, expected)) {
                    pass()
                } else {
                    fail(shown.to_owned())
                }
            }
            Assertion::NoCrash | Assertion::Log { .. } | Assertion::NoLog { .. } => {
                unreachable!("not a screen check: {self}")
            }
        }
    }
}

/// A target that isn't there fails; one that can't be told apart is an error in the check.
fn missing(err: Error, finding: &dyn Fn(Outcome, Option<String>) -> Finding) -> Finding {
    match err {
        Error::ElementNotFound { candidates, .. } if candidates.is_empty() => {
            finding(Outcome::Fail, Some("not on screen".into()))
        }
        Error::ElementNotFound { candidates, .. } => finding(
            Outcome::Fail,
            Some(format!("not on screen; closest: {}", candidates.join("; "))),
        ),
        Error::AmbiguousTarget { candidates, .. } => finding(
            Outcome::Error,
            Some(format!(
                "matches several elements; use a more specific target: {}",
                candidates.join("; ")
            )),
        ),
        other => finding(Outcome::Error, Some(other.to_string())),
    }
}

/// `dev.app/.LoginActivity` is `.LoginActivity`, `LoginActivity` and `dev.app/dev.app.LoginActivity`.
fn same_screen(activity: &str, expected: &str) -> bool {
    let full = |a: &str| -> String {
        match a.split_once('/') {
            Some((package, class)) if class.starts_with('.') => format!("{package}{class}"),
            Some((_, class)) => class.to_owned(),
            None => a.to_owned(),
        }
    };
    let simple = |a: &str| full(a).rsplit('.').next().unwrap_or_default().to_owned();
    if expected.contains('/') {
        full(activity) == full(expected)
    } else if expected.trim_start_matches('.').contains('.') {
        full(activity) == expected
    } else {
        simple(activity) == expected.trim_start_matches('.')
    }
}

fn split_word(s: &str) -> (&str, &str) {
    match s.split_once(char::is_whitespace) {
        Some((head, rest)) => (head, rest.trim()),
        None => (s, ""),
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(s)
}

fn invalid(s: &str, reason: &str) -> Error {
    Error::InvalidAssertion {
        assertion: s.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> String {
        Assertion::parse(s)
            .map(|a| a.to_string())
            .unwrap_or_else(|e| format!("error: {e}"))
    }

    #[test]
    fn inline_forms_round_trip() {
        for (input, shown) in [
            ("visible Sign in", r#"visible "Sign in""#),
            (r#"visible "Sign in""#, r#"visible "Sign in""#),
            ("not visible id=error", "not visible id=error"),
            ("not_visible id=error", "not visible id=error"),
            ("gone role=progress", "not visible role=progress"),
            ("enabled id=sign_in", "enabled id=sign_in"),
            (
                "checked role=switch;text=Wi-Fi",
                "checked role=switch;text=Wi-Fi",
            ),
            (
                r#"text id=greeting == "Hello, Bob""#,
                r#"text id=greeting == "Hello, Bob""#,
            ),
            ("text id=greeting ~= bob", r#"text id=greeting ~= "bob""#),
            ("text text~=Hello ~= Bob", r#"text text~=Hello ~= "Bob""#),
            ("screen .MessagesActivity", "screen .MessagesActivity"),
            ("no crash", "no crash"),
            ("no-crash", "no crash"),
            ("log ~= timeout", r#"log ~= "timeout""#),
            ("no log ~= FATAL", r#"no log ~= "FATAL""#),
        ] {
            assert_eq!(parse(input), shown, "{input}");
            assert_eq!(parse(shown), shown, "{shown} parses back to itself");
        }
        assert!(parse("visible").starts_with("error"));
        assert!(parse("shiny id=x").starts_with("error"));
        assert!(parse("text id=x").starts_with("error"));
    }

    #[test]
    fn screens_match_short_and_full_names() {
        let a = "dev.mdh.sample/.LoginActivity";
        assert!(same_screen(a, ".LoginActivity"));
        assert!(same_screen(a, "LoginActivity"));
        assert!(same_screen(a, "dev.mdh.sample/.LoginActivity"));
        assert!(same_screen(a, "dev.mdh.sample.LoginActivity"));
        assert!(!same_screen(a, ".MainActivity"));
        assert!(!same_screen(a, "Login"));
        assert!(same_screen(
            "com.android.settings/com.android.settings.SubSettings",
            ".SubSettings"
        ));
    }

    #[test]
    fn json_forms() {
        let a: Vec<Assertion> = serde_json::from_str(
            r#"[{"visible": "id=inbox"}, {"enabled": {"text": "Sign in", "role": "button"}},
               {"text": {"target": "id=greeting", "equals": "Hi"}}, "no_crash",
               {"log": {"contains": "timeout", "level": "error"}}]"#,
        )
        .unwrap();
        let shown: Vec<String> = a.iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                "visible id=inbox",
                "enabled role=button;text=Sign in",
                r#"text id=greeting == "Hi""#,
                "no crash",
                r#"log ~= "timeout""#
            ]
        );
    }
}
