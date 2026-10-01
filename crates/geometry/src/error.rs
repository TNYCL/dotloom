use thiserror::Error;

/// Errors produced by geometry operations.
///
/// Geometry functions never panic on bad input; they return one of these explicit
/// results instead (non-finite numbers, degenerate shapes, unsupported transforms, ...).
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum GeometryError {
    /// An input coordinate or parameter was NaN or infinite.
    #[error("non-finite value in {0}")]
    NonFinite(&'static str),
    /// The shape has no extent where one is required (zero-length segment, zero radius, ...).
    #[error("degenerate geometry: {0}")]
    Degenerate(&'static str),
    /// The transform has a (near) zero determinant and cannot be inverted.
    #[error("singular transform (determinant {determinant:e})")]
    SingularTransform {
        /// Determinant of the linear part.
        determinant: f64,
    },
    /// The transform cannot be applied while keeping the shape's semantics
    /// (for example a non-uniform scale applied to a circle).
    #[error("unsupported transform for {shape}: {reason}")]
    UnsupportedTransform {
        /// Shape kind name.
        shape: &'static str,
        /// Human readable reason.
        reason: &'static str,
    },
    /// The requested operation is not defined for this shape kind.
    #[error("operation `{operation}` is not supported for {shape}")]
    UnsupportedOperation {
        /// Operation name.
        operation: &'static str,
        /// Shape kind name.
        shape: &'static str,
    },
    /// No intersection or boundary was found where the operation needs one.
    #[error("no boundary found: {0}")]
    NoBoundary(&'static str),
    /// A parameter is outside its valid range.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    /// Two quantities with different physical dimensions were combined.
    #[error("dimension mismatch: {left} vs {right}")]
    DimensionMismatch {
        /// Left operand dimension.
        left: String,
        /// Right operand dimension.
        right: String,
    },
    /// A unit string could not be parsed.
    #[error("unknown unit `{0}`")]
    UnknownUnit(String),
}

/// Result alias for geometry operations.
pub type GeoResult<T> = Result<T, GeometryError>;

pub(crate) fn finite(v: f64, what: &'static str) -> GeoResult<f64> {
    if v.is_finite() { Ok(v) } else { Err(GeometryError::NonFinite(what)) }
}
