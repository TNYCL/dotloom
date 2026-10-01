//! Canonical form and content hash.
//!
//! Two documents are semantically equal when their canonical JSON is equal:
//! object keys sorted, `-0.0` normalized to `0.0`, entities in draw order (draw
//! order is semantic), every other collection sorted by ID. Map iteration order
//! never influences the result.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{DocError, Document};

fn normalize(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let sorted: BTreeMap<String, Value> = m.into_iter().map(|(k, v)| (k, normalize(v))).collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f == 0.0 && n.is_f64() => serde_json::json!(0.0),
            _ => Value::Number(n),
        },
        other => other,
    }
}

/// FNV-1a 64-bit hash.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl Document {
    /// Canonical JSON value.
    pub fn canonical_value(&self) -> Result<Value, DocError> {
        let v = serde_json::to_value(self).map_err(|e| DocError::Malformed(e.to_string()))?;
        Ok(normalize(v))
    }

    /// Canonical JSON string (compact).
    pub fn canonical_json(&self) -> Result<String, DocError> {
        let v = self.canonical_value()?;
        serde_json::to_string(&v).map_err(|e| DocError::Malformed(e.to_string()))
    }

    /// Content hash of the canonical form, as 16 hex digits.
    pub fn content_hash(&self) -> Result<String, DocError> {
        Ok(format!("{:016x}", fnv1a64(self.canonical_json()?.as_bytes())))
    }

    /// Semantic equality (canonical forms equal).
    #[must_use]
    pub fn semantic_eq(&self, other: &Self) -> bool {
        match (self.canonical_json(), other.canonical_json()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}
