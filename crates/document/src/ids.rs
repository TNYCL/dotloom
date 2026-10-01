//! Stable identifiers.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::DocError;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

id_type!(
    /// Entity identifier. Stable for the lifetime of the entity; never reused within
    /// a document (allocated from `Document::next_id`).
    EntityId,
    "e"
);
id_type!(
    /// Constraint identifier.
    ConstraintId,
    "c"
);
id_type!(
    /// Layer identifier.
    LayerId,
    "l"
);
id_type!(
    /// Group identifier.
    GroupId,
    "g"
);

/// Namespaced entity type identifier such as `dotloom.line` or `acme.wall`.
///
/// Grammar: two or more dot-separated segments of `[a-z][a-z0-9_-]*`, at most 128
/// bytes in total.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TypeId(pub(crate) String);

impl TypeId {
    /// Maximum length in bytes.
    pub const MAX_LEN: usize = 128;

    /// Parse and validate.
    pub fn new(s: impl Into<String>) -> Result<Self, DocError> {
        let s = s.into();
        if Self::is_valid(&s) { Ok(Self(s)) } else { Err(DocError::InvalidTypeId(s)) }
    }

    /// Whether `s` is a valid type ID.
    #[must_use]
    pub fn is_valid(s: &str) -> bool {
        if s.is_empty() || s.len() > Self::MAX_LEN {
            return false;
        }
        let mut segments = 0;
        for seg in s.split('.') {
            segments += 1;
            let mut chars = seg.chars();
            match chars.next() {
                Some(c) if c.is_ascii_lowercase() => {}
                _ => return false,
            }
            if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
                return false;
            }
        }
        segments >= 2
    }

    /// The namespace (first segment).
    #[must_use]
    pub fn namespace(&self) -> &str {
        self.0.split('.').next().unwrap_or("")
    }

    /// As string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TypeId {
    type Error = DocError;
    fn try_from(s: String) -> Result<Self, DocError> {
        Self::new(s)
    }
}

impl From<TypeId> for String {
    fn from(t: TypeId) -> Self {
        t.0
    }
}

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_id_grammar() {
        assert!(TypeId::new("dotloom.line").is_ok());
        assert!(TypeId::new("acme.floor-plan.wall_2").is_ok());
        for bad in ["line", "Dotloom.line", "acme..wall", "acme.1wall", "", "acme.wa ll", "acme.wall."] {
            assert!(TypeId::new(bad).is_err(), "{bad}");
        }
        assert!(TypeId::new(format!("a.{}", "b".repeat(200))).is_err());
        assert_eq!(TypeId::new("acme.wall").unwrap().namespace(), "acme");
    }

    #[test]
    fn ids_serialize_as_numbers() {
        assert_eq!(serde_json::to_string(&EntityId(7)).unwrap(), "7");
        assert_eq!(EntityId(7).to_string(), "e7");
        let t: Result<TypeId, _> = serde_json::from_str("\"Bad\"");
        assert!(t.is_err());
    }
}
