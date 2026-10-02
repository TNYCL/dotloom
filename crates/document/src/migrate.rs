//! Document schema versions and migrations.
//!
//! The schema version is independent from package versions and the protocol
//! version. Each migration step transforms the raw JSON of schema `n` into schema
//! `n + 1`; unknown fields are carried through untouched.

use serde_json::Value;

use crate::{DocError, Document, Limits};

/// Current document schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// Oldest schema version that can be migrated.
pub const OLDEST_SUPPORTED_SCHEMA: u32 = 1;

/// A note produced while migrating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationNote {
    /// Schema the step started from.
    pub from: u32,
    /// Human readable description.
    pub message: String,
}

/// Schema version of a raw document.
pub fn schema_of(v: &Value) -> Result<u32, DocError> {
    let s =
        v.get("schema").and_then(Value::as_u64).ok_or_else(|| DocError::Malformed("missing `schema` field".into()))?;
    u32::try_from(s).map_err(|_| DocError::Malformed("schema out of range".into()))
}

type Step = fn(Value) -> Result<(Value, String), DocError>;

/// Migration steps indexed by source version (`STEPS[i]` migrates `OLDEST + i`).
const STEPS: &[Step] = &[];

/// Migrate a raw document to [`SCHEMA_VERSION`].
pub fn migrate(mut v: Value) -> Result<(Value, Vec<MigrationNote>), DocError> {
    let mut schema = schema_of(&v)?;
    if schema > SCHEMA_VERSION {
        return Err(DocError::FutureSchema { found: schema, supported: SCHEMA_VERSION });
    }
    if schema < OLDEST_SUPPORTED_SCHEMA {
        return Err(DocError::UnsupportedSchema(schema));
    }
    let mut notes = Vec::new();
    while schema < SCHEMA_VERSION {
        let idx = (schema - OLDEST_SUPPORTED_SCHEMA) as usize;
        let step = STEPS.get(idx).ok_or(DocError::UnsupportedSchema(schema))?;
        let (nv, message) = step(v)?;
        notes.push(MigrationNote { from: schema, message });
        v = nv;
        schema += 1;
        if let Some(obj) = v.as_object_mut() {
            obj.insert("schema".into(), Value::from(schema));
        }
    }
    Ok((v, notes))
}

/// Maximum JSON nesting depth accepted for documents.
pub const MAX_JSON_DEPTH: usize = 128;

fn depth(v: &Value, d: usize) -> usize {
    if d > MAX_JSON_DEPTH {
        return d;
    }
    match v {
        Value::Array(a) => a.iter().map(|x| depth(x, d + 1)).max().unwrap_or(d + 1),
        Value::Object(o) => o.values().map(|x| depth(x, d + 1)).max().unwrap_or(d + 1),
        _ => d,
    }
}

impl Document {
    /// Parse, migrate and validate a document from JSON text.
    ///
    /// Documents of the current schema are deserialized directly; only older
    /// schemas go through the JSON tree the migration steps work on (the tree costs
    /// several times the document's own memory: ~150 MB for 100 000 shapes). Nesting
    /// depth stays bounded by `serde_json`'s recursion limit (128) on both paths.
    pub fn from_json_str(s: &str, limits: &Limits) -> Result<(Self, Vec<MigrationNote>), DocError> {
        Self::from_json_slice(s.as_bytes(), limits)
    }

    /// [`Document::from_json_str`] on UTF-8 bytes.
    pub fn from_json_slice(bytes: &[u8], limits: &Limits) -> Result<(Self, Vec<MigrationNote>), DocError> {
        #[derive(serde::Deserialize)]
        struct Peek {
            schema: Option<u64>,
        }
        let peek: Peek = serde_json::from_slice(bytes).map_err(|e| DocError::Malformed(e.to_string()))?;
        if peek.schema == Some(u64::from(SCHEMA_VERSION)) {
            let doc: Self = serde_json::from_slice(bytes).map_err(|e| DocError::Malformed(e.to_string()))?;
            if let Some(first) = doc.validate_all(limits).into_iter().next() {
                return Err(first);
            }
            return Ok((doc, Vec::new()));
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|e| DocError::Malformed(e.to_string()))?;
        Self::from_json_value(v, limits)
    }

    /// Migrate and validate a parsed JSON value.
    pub fn from_json_value(v: Value, limits: &Limits) -> Result<(Self, Vec<MigrationNote>), DocError> {
        if depth(&v, 0) > MAX_JSON_DEPTH {
            return Err(DocError::LimitExceeded(format!("JSON nesting deeper than {MAX_JSON_DEPTH}")));
        }
        let (v, notes) = migrate(v)?;
        let doc: Self = serde_json::from_value(v).map_err(|e| DocError::Malformed(e.to_string()))?;
        if let Some(first) = doc.validate_all(limits).into_iter().next() {
            return Err(first);
        }
        Ok((doc, notes))
    }

    /// Serialize to pretty JSON.
    pub fn to_json_string(&self) -> Result<String, DocError> {
        serde_json::to_string_pretty(self).map_err(|e| DocError::Malformed(e.to_string()))
    }
}
