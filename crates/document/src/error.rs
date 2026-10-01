use thiserror::Error;

use crate::{ConstraintId, EntityId, GroupId, LayerId};

/// Document-level errors (invariant violations, invalid edits, limits).
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum DocError {
    /// Malformed type identifier.
    #[error("invalid type id `{0}`")]
    InvalidTypeId(String),
    /// The same ID appears twice.
    #[error("duplicate id {0}")]
    DuplicateId(String),
    /// An entity does not exist.
    #[error("unknown entity {0}")]
    UnknownEntity(EntityId),
    /// A constraint does not exist.
    #[error("unknown constraint {0}")]
    UnknownConstraint(ConstraintId),
    /// A layer does not exist.
    #[error("unknown layer {0}")]
    UnknownLayer(LayerId),
    /// A group does not exist.
    #[error("unknown group {0}")]
    UnknownGroup(GroupId),
    /// A reference points to a missing target.
    #[error("{from} references missing {target}")]
    BrokenReference {
        /// Referencing object.
        from: String,
        /// Missing target.
        target: String,
    },
    /// An anchor name is not provided by the referenced entity.
    #[error("entity {entity} has no anchor `{anchor}`")]
    MissingAnchor {
        /// Entity.
        entity: EntityId,
        /// Anchor name.
        anchor: String,
    },
    /// A parameter name is not provided by the referenced entity.
    #[error("entity {entity} has no numeric parameter `{param}`")]
    MissingParam {
        /// Entity.
        entity: EntityId,
        /// Parameter name.
        param: String,
    },
    /// Group nesting forms a cycle.
    #[error("group cycle through {0}")]
    GroupCycle(GroupId),
    /// An entity or group belongs to more than one group.
    #[error("{0} is a member of more than one group")]
    MultipleParents(String),
    /// The entity order list is not a permutation of the entities.
    #[error("entity order is inconsistent: {0}")]
    BadOrder(String),
    /// Geometry is invalid.
    #[error("invalid geometry for {entity}: {reason}")]
    InvalidGeometry {
        /// Entity.
        entity: EntityId,
        /// Reason.
        reason: String,
    },
    /// A value is invalid.
    #[error("invalid value: {0}")]
    InvalidValue(String),
    /// A configured limit was exceeded.
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
    /// The document schema is newer than this library supports.
    #[error("document schema {found} is newer than supported schema {supported}; upgrade Dotloom to open it")]
    FutureSchema {
        /// Schema found in the file.
        found: u32,
        /// Newest supported schema.
        supported: u32,
    },
    /// The document schema is older than any supported migration.
    #[error("document schema {0} is not supported")]
    UnsupportedSchema(u32),
    /// JSON (de)serialization error.
    #[error("malformed document: {0}")]
    Malformed(String),
}
