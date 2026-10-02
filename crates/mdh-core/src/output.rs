//! The envelope shared by `--json` CLI output and MCP structured results (ADR-0005).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{Error, ErrorCode};

pub const SCHEMA: &str = "mdh/v1";

#[derive(Debug, Serialize)]
pub struct Envelope<T> {
    pub schema: &'static str,
    pub ok: bool,
    /// Present on success. May also accompany an error when partial results are useful,
    /// e.g. the individual checks of a failing `doctor`.
    pub data: Option<T>,
    pub error: Option<ErrorBody>,
    pub warnings: Vec<String>,
    pub timing_ms: BTreeMap<&'static str, u64>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub hint: String,
}

impl From<&Error> for ErrorBody {
    fn from(e: &Error) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
            hint: e.hint(),
        }
    }
}

impl<T> Envelope<T> {
    pub fn new(data: Option<T>, error: Option<&Error>) -> Self {
        Self {
            schema: SCHEMA,
            ok: error.is_none(),
            data,
            error: error.map(ErrorBody::from),
            warnings: Vec::new(),
            timing_ms: BTreeMap::new(),
        }
    }

    pub fn timing(mut self, phase: &'static str, ms: u64) -> Self {
        self.timing_ms.insert(phase, ms);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_shape() {
        let err = Error::ToolNotFound {
            name: "adb".into(),
            hint: "install platform-tools".into(),
        };
        let env = Envelope::<()>::new(None, Some(&err)).timing("total", 12);
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "schema": "mdh/v1",
                "ok": false,
                "data": null,
                "error": {
                    "code": "TOOL_NOT_FOUND",
                    "message": "`adb` not found",
                    "hint": "install platform-tools"
                },
                "warnings": [],
                "timing_ms": { "total": 12 }
            })
        );
    }
}
