//! Deterministic elementary functions.
//!
//! `f64::sin` and friends call the platform's math library, whose results differ in
//! the last bit between Windows, Linux, macOS and WebAssembly. Everything that feeds
//! stored geometry, solver iterations, exported files or the scene goes through these
//! wrappers (the pure-Rust `libm`), so the same input gives the same bits on every
//! platform (DL-TEST-6). `sqrt` is correctly rounded everywhere and stays `f64::sqrt`.

/// Sine.
#[inline]
#[must_use]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// Cosine.
#[inline]
#[must_use]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// `(sin x, cos x)`.
#[inline]
#[must_use]
pub fn sin_cos(x: f64) -> (f64, f64) {
    libm::sincos(x)
}

/// Tangent.
#[inline]
#[must_use]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

/// Arctangent.
#[inline]
#[must_use]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// Four-quadrant arctangent of `y / x`.
#[inline]
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// Arccosine.
#[inline]
#[must_use]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// `sqrt(x² + y²)` without undue overflow.
#[inline]
#[must_use]
pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// Base-2 logarithm.
#[inline]
#[must_use]
pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::disallowed_methods)] // compares against the platform functions on purpose
    fn wrappers_agree_with_std_within_two_ulps() {
        for i in -200..200 {
            let x = f64::from(i) * 0.0731;
            let close = |a: f64, b: f64| (a - b).abs() <= 2.0 * f64::EPSILON * a.abs().max(b.abs()).max(1.0);
            assert!(close(sin(x), x.sin()));
            assert!(close(cos(x), x.cos()));
            assert!(close(atan2(x, 1.3), x.atan2(1.3)));
            assert!(close(hypot(x, 2.0), x.hypot(2.0)));
            assert_eq!(sin_cos(x), (sin(x), cos(x)));
        }
    }
}
