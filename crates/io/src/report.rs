//! Conversion (import/export) loss reports.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What kind of information was lost or changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LossKind {
    /// Constraints are not representable in the target format.
    Constraints,
    /// Plugin entities exported as plain geometry (parameters lost).
    PluginGeometry,
    /// Associative dimensions exported as static geometry.
    Dimensions,
    /// Geometry approximated (curves flattened, ellipses as Béziers, ...).
    Approximated,
    /// Elements or entities that are not supported and were skipped.
    Unsupported,
    /// Styling simplified (CSS, gradients, line types, ...).
    Style,
    /// Text formatting simplified.
    Text,
    /// External references were ignored (never fetched).
    ExternalReference,
    /// Unit or scale assumption.
    Units,
    /// Read-only entities exported from their fallback representation.
    Fallback,
}

/// One loss line with an occurrence count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loss {
    /// Category.
    pub kind: LossKind,
    /// Detail (element or entity type).
    pub what: String,
    /// Occurrences.
    pub count: usize,
}

/// Report attached to every import and export.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionReport {
    /// Entities read or written.
    pub entities: usize,
    /// Losses grouped by kind and detail.
    pub losses: Vec<Loss>,
    /// Informational notes (versions, units, ...).
    pub notes: Vec<String>,
}

/// Accumulates losses.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    counts: BTreeMap<(LossKind, String), usize>,
    pub notes: Vec<String>,
    pub entities: usize,
}

impl Recorder {
    pub fn add(&mut self, kind: LossKind, what: impl Into<String>) {
        *self.counts.entry((kind, what.into())).or_insert(0) += 1;
    }

    pub fn finish(self) -> ConversionReport {
        ConversionReport {
            entities: self.entities,
            losses: self.counts.into_iter().map(|((kind, what), count)| Loss { kind, what, count }).collect(),
            notes: self.notes,
        }
    }
}

impl ConversionReport {
    /// Whether nothing was lost.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.losses.is_empty()
    }

    /// Total count for a kind.
    #[must_use]
    pub fn count(&self, kind: LossKind) -> usize {
        self.losses.iter().filter(|l| l.kind == kind).map(|l| l.count).sum()
    }
}
