//! YAML behind one seam (architecture §9): `serde_yaml` is unmaintained; `serde_norway` is its
//! maintained fork and can be swapped here without touching the rest. Enums are written as
//! single-key maps (`- tap: Sign in`), not YAML tags (`- !tap Sign in`).

use mdh_core::{Error, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_norway::with::singleton_map_recursive;

pub fn from_str<T: DeserializeOwned>(what: &str, s: &str) -> Result<T> {
    singleton_map_recursive::deserialize(serde_norway::Deserializer::from_str(s)).map_err(|e| {
        Error::InvalidFlow {
            flow: what.to_owned(),
            reason: e.to_string(),
        }
    })
}

pub fn to_string<T: Serialize>(value: &T) -> String {
    let mut out = Vec::new();
    let mut serializer = serde_norway::Serializer::new(&mut out);
    singleton_map_recursive::serialize(value, &mut serializer).expect("flows are serializable");
    String::from_utf8(out).expect("YAML is UTF-8")
}
