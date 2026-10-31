// SPDX-License-Identifier: Apache-2.0
//! Elementary functions with the same bits on every target.
//!
//! `f64::sin` and its siblings call the platform's C library, and glibc, the
//! MSVC runtime, and Apple's libm round them differently. An adaptive
//! tessellation or a Newton projection can amplify that difference far beyond
//! the last place. [`Portable`] computes each function in Rust from IEEE-754
//! binary64 operations, whose results Rust fixes on every supported target.
//! Functions that IEEE-754 rounds exactly, such as `f64::sqrt`, need no
//! replacement.

/// The `f64` elementary functions of the `libm` crate, in method form.
pub trait Portable {
    /// The sine, in radians.
    fn portable_sin(self) -> f64;
    /// The cosine, in radians.
    fn portable_cos(self) -> f64;
    /// The sine and the cosine, in radians.
    fn portable_sin_cos(self) -> (f64, f64);
    /// The tangent, in radians.
    fn portable_tan(self) -> f64;
    /// The arcsine, in radians.
    fn portable_asin(self) -> f64;
    /// The arccosine, in radians.
    fn portable_acos(self) -> f64;
    /// The arctangent, in radians.
    fn portable_atan(self) -> f64;
    /// The four-quadrant arctangent of `self / x`, in radians.
    fn portable_atan2(self, x: f64) -> f64;
    /// The hyperbolic sine.
    fn portable_sinh(self) -> f64;
    /// The hyperbolic cosine.
    fn portable_cosh(self) -> f64;
    /// The hyperbolic tangent.
    fn portable_tanh(self) -> f64;
    /// The inverse hyperbolic sine.
    fn portable_asinh(self) -> f64;
    /// The inverse hyperbolic cosine.
    fn portable_acosh(self) -> f64;
    /// The inverse hyperbolic tangent.
    fn portable_atanh(self) -> f64;
    /// The natural exponential.
    fn portable_exp(self) -> f64;
    /// The natural exponential, minus one.
    fn portable_exp_m1(self) -> f64;
    /// The natural logarithm.
    fn portable_ln(self) -> f64;
    /// The length of the vector `(self, other)`.
    fn portable_hypot(self, other: f64) -> f64;
}

impl Portable for f64 {
    fn portable_sin(self) -> f64 {
        libm::sin(self)
    }

    fn portable_cos(self) -> f64 {
        libm::cos(self)
    }

    fn portable_sin_cos(self) -> (f64, f64) {
        libm::sincos(self)
    }

    fn portable_tan(self) -> f64 {
        libm::tan(self)
    }

    fn portable_asin(self) -> f64 {
        libm::asin(self)
    }

    fn portable_acos(self) -> f64 {
        libm::acos(self)
    }

    fn portable_atan(self) -> f64 {
        libm::atan(self)
    }

    fn portable_atan2(self, x: f64) -> f64 {
        libm::atan2(self, x)
    }

    fn portable_sinh(self) -> f64 {
        libm::sinh(self)
    }

    fn portable_cosh(self) -> f64 {
        libm::cosh(self)
    }

    fn portable_tanh(self) -> f64 {
        libm::tanh(self)
    }

    fn portable_asinh(self) -> f64 {
        libm::asinh(self)
    }

    fn portable_acosh(self) -> f64 {
        libm::acosh(self)
    }

    fn portable_atanh(self) -> f64 {
        libm::atanh(self)
    }

    fn portable_exp(self) -> f64 {
        libm::exp(self)
    }

    fn portable_exp_m1(self) -> f64 {
        libm::expm1(self)
    }

    fn portable_ln(self) -> f64 {
        libm::log(self)
    }

    fn portable_hypot(self, other: f64) -> f64 {
        libm::hypot(self, other)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_4, PI};

    use super::Portable;

    const EXACT: f64 = 1.0e-15;

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() <= EXACT, "{left} against {right}");
    }

    #[test]
    fn functions_match_their_closed_values() {
        close(0.0_f64.portable_sin(), 0.0);
        close((PI / 2.0).portable_sin(), 1.0);
        close(PI.portable_cos(), -1.0);
        close(FRAC_PI_4.portable_tan(), 1.0);
        close(1.0_f64.portable_asin(), PI / 2.0);
        close((-1.0_f64).portable_acos(), PI);
        close(1.0_f64.portable_atan(), FRAC_PI_4);
        close(1.0_f64.portable_atan2(-1.0), 3.0 * FRAC_PI_4);
        close(0.0_f64.portable_sinh(), 0.0);
        close(0.0_f64.portable_cosh(), 1.0);
        close(0.0_f64.portable_tanh(), 0.0);
        close(0.0_f64.portable_asinh(), 0.0);
        close(1.0_f64.portable_acosh(), 0.0);
        close(0.0_f64.portable_atanh(), 0.0);
        close(0.0_f64.portable_exp(), 1.0);
        close(0.0_f64.portable_exp_m1(), 0.0);
        close(1.0_f64.portable_ln(), 0.0);
        close(3.0_f64.portable_hypot(4.0), 5.0);
    }

    #[test]
    fn sin_cos_returns_the_sine_first() {
        let (sine, cosine) = FRAC_PI_4.portable_sin_cos();
        assert_eq!(sine.to_bits(), FRAC_PI_4.portable_sin().to_bits());
        assert_eq!(cosine.to_bits(), FRAC_PI_4.portable_cos().to_bits());
    }
}
