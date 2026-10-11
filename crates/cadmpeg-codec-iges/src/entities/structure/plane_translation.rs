// SPDX-License-Identifier: Apache-2.0
//! Exact subtraction bounds for cached axis-aligned boundary projections.

use cadmpeg_ir::math::{Point3, Vector3};

/// Dyadic spacing and magnitude of the sampled world coordinates.
#[derive(Clone, Copy, Default)]
pub(super) struct TranslationLattice {
    spacing: [Option<f64>; 3],
    magnitude: [f64; 3],
}

fn dyadic_spacing(value: f64) -> Option<f64> {
    if value == 0.0 || !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let exponent = (bits >> 52) & 0x7ff;
    let significand = (bits & ((1 << 52) - 1)) | if exponent == 0 { 0 } else { 1 << 52 };
    let shift = u64::from(significand.trailing_zeros());
    let spacing_bits = if exponent == 0 {
        1 << shift
    } else if exponent + shift > 52 {
        (exponent + shift - 52) << 52
    } else {
        1 << (exponent + shift - 1)
    };
    Some(f64::from_bits(spacing_bits))
}

fn finer(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

impl TranslationLattice {
    /// Records a point during the admitted duplicate comparisons.
    pub(super) fn record(&mut self, point: Point3) {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            self.spacing[axis] = finer(self.spacing[axis], dyadic_spacing(value));
            self.magnitude[axis] = self.magnitude[axis].max(value.abs());
        }
    }

    /// An unchanged axis frame and exact coordinate differences preserve every
    /// orientation, coordinate ordering and endpoint equality, including zeros.
    pub(super) fn preserves_projection(
        self,
        previous: (Point3, Vector3),
        current: (Point3, Vector3),
    ) -> bool {
        let components = [previous.1.x, previous.1.y, previous.1.z];
        if previous.1 != current.1
            || components.iter().filter(|value| value.abs() == 1.0).count() != 1
            || components.iter().any(|value| *value != 0.0 && value.abs() != 1.0)
            || !previous.0.is_finite()
            || !current.0.is_finite()
        {
            return false;
        }
        let old = [previous.0.x, previous.0.y, previous.0.z];
        let new = [current.0.x, current.0.y, current.0.z];
        (0..3).all(|axis| {
            let magnitude = self.magnitude[axis].max(old[axis].abs()).max(new[axis].abs());
            if magnitude > f64::MAX / 4.0 {
                return false;
            }
            let spacing = finer(finer(self.spacing[axis], dyadic_spacing(old[axis])), dyadic_spacing(new[axis]));
            // All coordinates and origins are integer multiples of spacing.
            // Their magnitudes use at most 51 bits, so two subtractions fit
            // within the 53-bit significand without rounding or overflow.
            spacing.is_none_or(|spacing| magnitude / spacing <= 2.0_f64.powi(50))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dyadic_spacing_covers_normal_and_subnormal_significands() {
        for (value, spacing) in [
            (1.0, 1.0), (3.0, 1.0), (0.75, 0.25),
            (2.0_f64.powi(-1022), 2.0_f64.powi(-1022)),
            (f64::from_bits(3), f64::from_bits(1)),
            (f64::from_bits(1 << 51), f64::from_bits(1 << 51)),
            (f64::MAX, 2.0_f64.powi(971)),
        ] {
            assert_eq!(dyadic_spacing(value), Some(spacing));
            assert_eq!(dyadic_spacing(-value), Some(spacing));
        }
        for value in [0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(dyadic_spacing(value), None);
        }
    }

    #[test]
    fn exact_translation_keeps_projected_differences_for_every_axis_frame() {
        let points = [Point3::new(-2.0, 0.25, 0.0), Point3::new(3.5, 2.0, -1.0), Point3::new(1.0, 4.0, 2.5)];
        let mut lattice = TranslationLattice::default();
        for point in points { lattice.record(point); }
        crate::test_support::with_service_context(&[], |ctx| {
            for normal in [Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 0.0, 1.0)] {
                for normal in [normal, normal.scale(-1.0)] {
                    let previous = (Point3::new(0.0, 0.0, 0.0), normal);
                    let before = crate::entities::geometry::plane_coordinates(&points, previous, ctx).expect("projection admission").expect("finite axis projection");
                    for offset in [-1024.0, -1.0, 0.25, 1.0, 1024.0] {
                        let current = (Point3::new(offset, -offset, offset / 2.0), normal);
                        assert!(lattice.preserves_projection(previous, current));
                        let after = crate::entities::geometry::plane_coordinates(&points, current, ctx).expect("projection admission").expect("finite axis projection");
                        let difference = |first: [f64; 2], second: [f64; 2]| [first[0] - second[0], first[1] - second[1]];
                        for (before_first, after_first) in before.iter().zip(&after) {
                            for (before_second, after_second) in before.iter().zip(&after) {
                                assert_eq!(difference(*before_first, *before_second), difference(*after_first, *after_second));
                            }
                        }
                    }
                }
            }
        });
    }

    #[test]
    fn translation_lattice_rejects_rounding_and_overflow() {
        let normal = Vector3::new(0.0, 0.0, 1.0);
        let original = (Point3::new(0.0, 0.0, 0.0), normal);
        let mut lattice = TranslationLattice::default();
        lattice.record(Point3::new(1.0, 2.0_f64.powi(-40), 0.0));
        assert!(!lattice.preserves_projection(original, (Point3::new(0.0, 100_000.0, 0.0), normal)));
        assert!(!lattice.preserves_projection(original, (Point3::new(f64::MAX, 0.0, 0.0), normal)));
        assert!(!lattice.preserves_projection(original, (original.0, normal.scale(2.0))));
    }
}
