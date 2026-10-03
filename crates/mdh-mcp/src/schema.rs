//! Compact tool schemas. Every tool definition is sent with every request the agent makes, so its
//! size is paid on every turn. The generated schemas carry what a model doesn't use — integer
//! formats and bounds, `null` alongside every optional type, references into `$defs` — and those
//! go; names, types, enums, required fields and descriptions stay.

use serde_json::{Map, Value};

/// Keys that describe representation, not meaning.
const DROPPED: &[&str] = &[
    "$schema", "title", "format", "minimum", "maximum", "minItems", "maxItems",
];

pub fn compact(schema: &Map<String, Value>) -> Map<String, Value> {
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match walk(&Value::Object(schema.clone()), &defs) {
        Value::Object(mut m) => {
            m.remove("$defs");
            m.remove("definitions");
            m
        }
        _ => unreachable!("an object stays an object"),
    }
}

fn walk(v: &Value, defs: &Map<String, Value>) -> Value {
    match v {
        Value::Object(m) => {
            // Inline references: the definitions are small enums and structs.
            if let Some(name) = m
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.rsplit('/').next())
                && let Some(def) = defs.get(name)
            {
                let mut inlined = match walk(def, defs) {
                    Value::Object(d) => d,
                    other => return other,
                };
                // A description next to the reference wins over the definition's.
                if let Some(d) = m.get("description") {
                    inlined.insert("description".into(), d.clone());
                }
                return Value::Object(inlined);
            }
            let mut out = Map::new();
            for (k, v) in m {
                if DROPPED.contains(&k.as_str()) || k == "$defs" || k == "definitions" {
                    continue;
                }
                let v = match (k.as_str(), v) {
                    // `["string", "null"]` → `"string"`: optional fields aren't required anyway.
                    ("type", Value::Array(types)) => {
                        let kept: Vec<&Value> = types
                            .iter()
                            .filter(|t| t.as_str() != Some("null"))
                            .collect();
                        match kept.as_slice() {
                            [one] => (*one).clone(),
                            _ => Value::Array(kept.into_iter().cloned().collect()),
                        }
                    }
                    // `anyOf: [X, {type: null}]` → X.
                    ("anyOf", Value::Array(alts))
                        if alts.len() == 2 && alts.iter().any(is_null) =>
                    {
                        let other = alts.iter().find(|a| !is_null(a)).expect("one isn't null");
                        if let Value::Object(o) = walk(other, defs) {
                            for (ok, ov) in o {
                                out.entry(ok).or_insert(ov);
                            }
                        }
                        continue;
                    }
                    _ => walk(v, defs),
                };
                out.insert(k.clone(), v);
            }
            // `oneOf` of bare constants → an enum.
            if let Some(Value::Array(alts)) = out.get("oneOf")
                && !alts.is_empty()
                && alts.iter().all(|a| {
                    a.as_object().is_some_and(|o| {
                        o.contains_key("const") && o.keys().all(|k| k == "const" || k == "type")
                    })
                })
            {
                let values: Vec<Value> = alts.iter().map(|a| a["const"].clone()).collect();
                out.remove("oneOf");
                out.insert("type".into(), Value::String("string".into()));
                out.insert("enum".into(), Value::Array(values));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| walk(x, defs)).collect()),
        other => other.clone(),
    }
}

fn is_null(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("null")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_representation_keeps_meaning() {
        let schema = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "P",
            "type": "object",
            "properties": {
                "n": {"type": ["integer", "null"], "format": "uint32", "minimum": 0, "description": "Runs."},
                "dir": {"$ref": "#/$defs/Dir", "description": "Where to."},
                "level": {"anyOf": [{"$ref": "#/$defs/Level"}, {"type": "null"}]}
            },
            "required": ["dir"],
            "$defs": {
                "Dir": {"oneOf": [{"const": "up", "type": "string"}, {"const": "down", "type": "string"}]},
                "Level": {"type": "string", "enum": ["warn", "error"]}
            }
        });
        let out = Value::Object(compact(schema.as_object().unwrap()));
        assert_eq!(
            out,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "n": {"type": "integer", "description": "Runs."},
                    "dir": {"type": "string", "enum": ["up", "down"], "description": "Where to."},
                    "level": {"type": "string", "enum": ["warn", "error"]}
                },
                "required": ["dir"]
            })
        );
    }
}
