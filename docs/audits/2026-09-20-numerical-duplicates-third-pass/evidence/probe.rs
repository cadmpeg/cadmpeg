#![allow(dead_code,unused_imports)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{SolvedCurveGeometry,sampled::PolylineCurve,nurbs::{NurbsCurve,NurbsSurface,NurbsSurfaceAxis,NurbsSurfaceLanes}};
use cadmpeg_ir::sketches::{SketchGeometry,SketchGeometryDefinition};
use cadmpeg_ir::scalar::{Angle,Length};
use cadmpeg_ir::transform::Transform;
fn plane(a:f64,b:f64)->NurbsSurface {let axis=NurbsSurfaceAxis::new(1,vec![0.,0.,1.,1.],false);NurbsSurface::from_lanes(axis.clone(),axis,NurbsSurfaceLanes::new(vec![vec![Point3::new(0.,0.,0.),Point3::new(0.,b,0.)],vec![Point3::new(a,0.,0.),Point3::new(a,b,0.)]],None),false).unwrap()}
fn line(a:[f64;2],b:[f64;2])->SketchGeometry{SketchGeometryDefinition::Line{start:Point2::new(a[0],a[1]),end:Point2::new(b[0],b[1])}.try_into().unwrap()}
fn arc(c:[f64;2],r:f64)->SketchGeometry{SketchGeometryDefinition::Arc{center:Point2::new(c[0],c[1]),radius:Length::new(r).unwrap(),start_angle:Angle::new(0.).unwrap(),end_angle:Angle::new(std::f64::consts::TAU).unwrap()}.try_into().unwrap()}
mod math {pub use cadmpeg_ir::math::*;pub mod sum {// SPDX-License-Identifier: Apache-2.0
// Finite sums with an exact-product fallback for floating-point range loss.

/// A finite dot product with an exact-product fallback for range loss or cancellation.
pub(crate) fn finite_dot<const N: usize>(
    coefficients: [f64; N],
    components: [f64; N],
) -> Option<f64> {
    if coefficients
        .iter()
        .chain(&components)
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let products = std::array::from_fn(|index| coefficients[index] * components[index]);
    if let Some(value) = fast_dot(coefficients, components, products) {
        return Some(value);
    }
    let mut sum = ExactSignedSum::default();
    for (coefficient, component) in coefficients.into_iter().zip(components) {
        sum.add_product(coefficient, component);
    }
    let Some(value) = sum.finish() else {
        return Some(0.0);
    };
    value.finite()
}

// The smallest exponent a finite `f64` significand carries. Every exponent
// this module handles is biased by it, which keeps the biased value unsigned
// and makes the product shift below a `usize` with no conversion.
const MIN_SIGNIFICAND_EXPONENT: i32 = -1074;

// Four finite factors need at most 212 significand bits. The accumulator
// includes their entire exponent range, 64 low guard bits for rounded scaled
// terms, and enough high carry bits for any addressable collection of terms.
const EXACT_PRODUCT_EXPONENT: i32 = 4 * MIN_SIGNIFICAND_EXPONENT - 64;
const EXACT_SUM_WORDS: usize = 138;

#[derive(Clone, Copy)]
pub(crate) struct ExactSignedSum {
    positive: [u64; EXACT_SUM_WORDS],
    negative: [u64; EXACT_SUM_WORDS],
}

impl Default for ExactSignedSum {
    fn default() -> Self {
        Self {
            positive: [0; EXACT_SUM_WORDS],
            negative: [0; EXACT_SUM_WORDS],
        }
    }
}

/// The largest biased significand exponent a finite `f64` states. The field is
/// eleven bits wide and a finite value never reaches the all-ones field, and
/// biasing by [`MIN_SIGNIFICAND_EXPONENT`] subtracts one more.
const MAX_BIASED_SIGNIFICAND_EXPONENT: i32 = 0x7fe - 1;

/// The widest significand a finite `f64` states, the hidden bit included.
///
/// [`finite_significand`] proves it. Its subnormal branch answers a nonzero
/// `fraction`, which is below `1 << 52`, and its normal branch answers
/// `(1 << 52) | fraction`. The highest set bit is therefore at most 52 and the
/// width at most 53.
const MAX_SIGNIFICAND_BITS: i32 = 53;

/// The least power of two a [`ScaledExponent`] holds. `ExactSignedSum::finish`
/// states `EXACT_PRODUCT_EXPONENT + highest_bit + 1` with `highest_bit` at
/// least zero, which is below everything `scaled_finite` can state.
const MIN_SCALED_EXPONENT: i32 = EXACT_PRODUCT_EXPONENT + 1;

/// The greatest power of two a [`ScaledExponent`] holds.
/// `ExactSignedSum::finish` states `EXACT_PRODUCT_EXPONENT + highest_bit + 2`
/// in its rounding-carry arm, with `highest_bit` at most one below the
/// accumulator's bit width.
const MAX_SCALED_EXPONENT: i32 = EXACT_PRODUCT_EXPONENT + EXACT_SUM_WORDS as i32 * 64 + 1;

/// The power of two that scales a [`ScaledValue`]'s mantissa back to the value.
///
/// The field is private and the type has no constructor of its own, so the two
/// construction expressions in [`scaled_finite`] and [`ExactSignedSum::finish`]
/// are the only values that exist. Neither reads an exponent from outside, and
/// the four `const` assertions below state that both reach only
/// `MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScaledExponent(i32);

/// `scaled_finite` states `MIN_SIGNIFICAND_EXPONENT + biased + bits`, with
/// `biased` from zero to `MAX_BIASED_SIGNIFICAND_EXPONENT` and `bits` from one
/// to `MAX_SIGNIFICAND_BITS`.
const _: () = assert!(MIN_SIGNIFICAND_EXPONENT + 1 >= MIN_SCALED_EXPONENT);
const _: () = assert!(
    MIN_SIGNIFICAND_EXPONENT + MAX_BIASED_SIGNIFICAND_EXPONENT + MAX_SIGNIFICAND_BITS
        <= MAX_SCALED_EXPONENT
);

/// `ExactSignedSum::finish` states `EXACT_PRODUCT_EXPONENT + highest_bit + 1`,
/// and `+ 2` in its rounding-carry arm, with `highest_bit` indexing an
/// accumulator `EXACT_SUM_WORDS` words of 64 bits wide.
const _: () = assert!(EXACT_PRODUCT_EXPONENT + 1 >= MIN_SCALED_EXPONENT);
const _: () =
    assert!(EXACT_PRODUCT_EXPONENT + (EXACT_SUM_WORDS as i32 * 64 - 1) + 2 <= MAX_SCALED_EXPONENT);

impl ScaledExponent {
    /// The exponent shift that rescales a value in this frame into `other`'s.
    ///
    /// Plain `-`: both sides lie in `MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT`,
    /// so the difference is at most `MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT`
    /// in magnitude.
    fn difference(self, other: Self) -> i32 {
        self.0 - other.0
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScaledValue {
    sign: f64,
    mantissa: f64,
    pub(crate) exponent: ScaledExponent,
}

impl ScaledValue {
    pub(crate) fn exponent(self) -> i32 {
        self.exponent.0
    }

    pub(crate) fn rescale(self, exponent: i32) -> Option<f64> {
        scale_power_of_two(
            self.sign * self.mantissa,
            self.exponent.0.checked_sub(exponent)?,
        )
    }

    /// The value, rescaled into the frame whose exponent is `scale_exponent`.
    pub(crate) fn scaled_by(self, scale_exponent: ScaledExponent) -> f64 {
        self.sign * self.mantissa * 2.0_f64.powi(self.exponent.difference(scale_exponent))
    }
    pub(crate) fn finite(self) -> Option<f64> {
        scale_power_of_two(self.sign * self.mantissa, self.exponent.0)
    }

    pub(crate) fn quotient(self, denominator: Self) -> Option<f64> {
        scale_power_of_two(
            self.sign * denominator.sign * (self.mantissa / denominator.mantissa),
            self.exponent.difference(denominator.exponent),
        )
    }
}

/// The sign, the integer significand, and the significand's exponent biased by
/// [`MIN_SIGNIFICAND_EXPONENT`], of a finite `f64`.
///
/// The biased exponent is a `u16`: the field is eleven bits wide, and a finite
/// value never reaches the all-ones field, so the biased value is at most 2045.
fn finite_significand(value: f64) -> Option<(bool, u64, u16)> {
    if !value.is_finite() {
        return None;
    }
    let bits = value.to_bits();
    let negative = bits >> 63 != 0;
    // The mask keeps eleven bits, so the field is a `u16` by construction.
    let exponent_field = ((bits >> 52) & 0x7ff) as u16;
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent_field == 0 {
        (fraction != 0).then_some((negative, fraction, 0))
    } else {
        // A normal value's exponent is `field - 1075`, which is
        // `field - 1` once biased by `MIN_SIGNIFICAND_EXPONENT`.
        Some((negative, (1_u64 << 52) | fraction, exponent_field - 1))
    }
}

fn add_word(words: &mut [u64; EXACT_SUM_WORDS], index: usize, value: u64) {
    let (sum, mut carry) = words[index].overflowing_add(value);
    words[index] = sum;
    let mut index = index + 1;
    while carry {
        let (sum, next) = words[index].overflowing_add(1);
        words[index] = sum;
        carry = next;
        index += 1;
    }
}

fn add_shifted(words: &mut [u64; EXACT_SUM_WORDS], value: u128, shift: usize) {
    for (limb_index, limb) in [value as u64, (value >> 64) as u64].into_iter().enumerate() {
        if limb == 0 {
            continue;
        }
        let bit = shift + limb_index * 64;
        let word = bit / 64;
        let remainder = bit % 64;
        if remainder == 0 {
            add_word(words, word, limb);
        } else {
            add_word(words, word, limb << remainder);
            add_word(words, word + 1, limb >> (64 - remainder));
        }
    }
}

impl ExactSignedSum {
    pub(crate) fn add_product(&mut self, left: f64, right: f64) {
        self.add_factors([left, right]);
    }

    pub(crate) fn add_factors<const N: usize>(&mut self, factors: [f64; N]) {
        // All callers supply at most four factors, the tensor-product point case.
        assert!(N <= 4);
        let mut product = [1_u64, 0, 0, 0];
        let mut negative = false;
        let mut exponent = 0_i32;
        for factor in factors {
            let Some((sign, significand, biased)) = finite_significand(factor) else {
                return;
            };
            negative ^= sign;
            exponent += MIN_SIGNIFICAND_EXPONENT + i32::from(biased);
            let mut carry = 0_u128;
            for word in &mut product {
                let value = u128::from(*word) * u128::from(significand) + carry;
                *word = value as u64;
                carry = value >> 64;
            }
        }
        let shift = (exponent - EXACT_PRODUCT_EXPONENT) as usize;
        let target = if negative {
            &mut self.negative
        } else {
            &mut self.positive
        };
        add_shifted(
            target,
            u128::from(product[0]) | (u128::from(product[1]) << 64),
            shift,
        );
        add_shifted(
            target,
            u128::from(product[2]) | (u128::from(product[3]) << 64),
            shift + 128,
        );
    }

    /// Add a rounded extended-range sum times a finite coefficient. Rational
    /// quotient derivatives use this to subtract weight derivatives before division.
    pub(crate) fn add_scaled_product(
        &mut self,
        value: Option<ScaledValue>,
        factor: f64,
    ) -> Option<()> {
        if !factor.is_finite() {
            return None;
        }
        let Some(value) = value else {
            return Some(());
        };
        let Some((negative, significand, exponent)) =
            finite_significand(value.sign * value.mantissa)
        else {
            return Some(());
        };
        let Some((factor_negative, factor_significand, factor_exponent)) =
            finite_significand(factor)
        else {
            return Some(());
        };
        let product = u128::from(significand) * u128::from(factor_significand);
        let shift = value.exponent.0
            + 2 * MIN_SIGNIFICAND_EXPONENT
            + i32::from(exponent)
            + i32::from(factor_exponent)
            - EXACT_PRODUCT_EXPONENT;
        let shift = usize::try_from(shift).ok()?;
        if shift + 128 >= EXACT_SUM_WORDS * 64 {
            return None;
        }
        let target = if negative ^ factor_negative {
            &mut self.negative
        } else {
            &mut self.positive
        };
        add_shifted(target, product, shift);
        Some(())
    }

    pub(crate) fn finish(self) -> Option<ScaledValue> {
        let (negative, magnitude) = signed_difference(&self.positive, &self.negative)?;
        let word = magnitude.iter().rposition(|value| *value != 0)?;
        // `checked_ilog2` answers `None` for a zero word, which `rposition`
        // has already excluded; the `?` states that rather than asserting it.
        // `word` indexes `EXACT_SUM_WORDS` words of 64 bits, so a bit index of
        // the accumulator is a `u16` and converts to `i32` with no range check.
        let highest_bit = (word * 64 + magnitude[word].checked_ilog2()? as usize) as u16;
        let keep = (highest_bit + 1).min(53);
        let mut significand = 0_u64;
        for bit in (highest_bit + 1 - keep..=highest_bit).rev() {
            significand = (significand << 1) | u64::from(bit_is_set(&magnitude, usize::from(bit)));
        }
        if keep == 53 {
            let guard_bit = highest_bit
                .checked_sub(keep)
                .is_some_and(|bit| bit_is_set(&magnitude, usize::from(bit)));
            let sticky = highest_bit.checked_sub(keep).is_some_and(|bit| {
                (0..bit).any(|candidate| bit_is_set(&magnitude, usize::from(candidate)))
            });
            if guard_bit && (sticky || significand & 1 != 0) {
                significand += 1;
                if significand == 1_u64 << 53 {
                    return Some(ScaledValue {
                        sign: if negative { -1.0 } else { 1.0 },
                        mantissa: 0.5,
                        exponent: ScaledExponent(
                            EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 2,
                        ),
                    });
                }
            }
        }
        Some(ScaledValue {
            sign: if negative { -1.0 } else { 1.0 },
            mantissa: significand as f64 * 2.0_f64.powi(-i32::from(keep)),
            exponent: ScaledExponent(EXACT_PRODUCT_EXPONENT + i32::from(highest_bit) + 1),
        })
    }
}

/// Returns the sign and the magnitude of `positive - negative`, or `None` when
/// the two accumulators are equal.
///
/// The comparison that names the larger accumulator is the only route to the
/// subtraction below, so the minuend is never the smaller side and the loop
/// cannot leave a borrow in the highest word. Splitting the comparison from the
/// subtraction would state that relation as an assertion instead of holding it.
fn signed_difference(
    positive: &[u64; EXACT_SUM_WORDS],
    negative: &[u64; EXACT_SUM_WORDS],
) -> Option<(bool, [u64; EXACT_SUM_WORDS])> {
    let (larger, smaller, is_negative) =
        positive
            .iter()
            .zip(negative)
            .rev()
            .find_map(|(left, right)| match left.cmp(right) {
                std::cmp::Ordering::Greater => Some((positive, negative, false)),
                std::cmp::Ordering::Less => Some((negative, positive, true)),
                std::cmp::Ordering::Equal => None,
            })?;
    let mut magnitude = [0; EXACT_SUM_WORDS];
    let mut borrow = false;
    for index in 0..EXACT_SUM_WORDS {
        let (difference, first_borrow) = larger[index].overflowing_sub(smaller[index]);
        let (difference, second_borrow) = difference.overflowing_sub(u64::from(borrow));
        magnitude[index] = difference;
        borrow = first_borrow || second_borrow;
    }
    Some((is_negative, magnitude))
}

fn bit_is_set(words: &[u64; EXACT_SUM_WORDS], bit: usize) -> bool {
    words[bit / 64] & (1_u64 << (bit % 64)) != 0
}

pub(crate) fn scaled_finite(value: f64) -> Option<ScaledValue> {
    let (negative, significand, exponent) = finite_significand(value)?;
    // A finite significand is never zero: the subnormal branch requires a
    // nonzero fraction and the normal branch sets bit 52.
    let highest_bit = significand.checked_ilog2()?;
    // `finite_significand` answers a significand whose highest set bit is at
    // most 52, so `bits` is at most `MAX_SIGNIFICAND_BITS`. That is the bound
    // the `ScaledExponent` assertion above relies on.
    let bits = highest_bit as i32 + 1;
    Some(ScaledValue {
        sign: if negative { -1.0 } else { 1.0 },
        mantissa: significand as f64 * 2.0_f64.powi(-bits),
        exponent: ScaledExponent(i32::from(exponent) + MIN_SIGNIFICAND_EXPONENT + bits),
    })
}

/// `value / denominator`, multiplied by each factor through one quotient that
/// keeps the extended exponent range. A knot span below the normal range makes
/// the plain quotient overflow although every product here is finite, and one
/// quotient states the same rounding for every factor.
///
/// A zero value, and a zero denominator that a repeated knot states, make every
/// product zero. `None` states a non-finite input, or a product that no finite
/// `f64` holds.
pub(crate) fn scaled_ratio_products<const N: usize>(
    value: f64,
    denominator: f64,
    factors: [f64; N],
) -> Option<[f64; N]> {
    if value == 0.0 || denominator == 0.0 {
        return Some([0.0; N]);
    }
    let value = scaled_finite(value)?;
    let denominator = scaled_finite(denominator)?;
    // Both significands are in `[0.5, 1)`, so the ratio is in `(0.5, 2)` and
    // every product below stays normal until `scale_power_of_two` states it.
    let ratio = value.sign * denominator.sign * (value.mantissa / denominator.mantissa);
    // Plain `+`: the difference lies inside the `ScaledExponent` span and a
    // factor's exponent inside the `f64` range.
    let exponent = value.exponent.difference(denominator.exponent);
    let mut products = [0.0; N];
    for (product, factor) in products.iter_mut().zip(factors) {
        if factor == 0.0 {
            continue;
        }
        let factor = scaled_finite(factor)?;
        *product = scale_power_of_two(
            ratio * factor.sign * factor.mantissa,
            exponent + factor.exponent.0,
        )?;
    }
    Some(products)
}

pub(crate) fn fast_dot<const N: usize>(
    coefficients: [f64; N],
    components: [f64; N],
    products: [f64; N],
) -> Option<f64> {
    if products.iter().any(|product| !product.is_finite()) {
        return None;
    }
    if coefficients.into_iter().zip(components).zip(products).any(
        |((coefficient, component), product)| {
            // A product below the normal range may have rounded before it
            // reached this check, even when it is nonzero. Recompute every
            // such column from the exact significands.
            coefficient != 0.0 && component != 0.0 && product.abs() < f64::MIN_POSITIVE
        },
    ) {
        return None;
    }
    let scale = products
        .iter()
        .fold(0.0_f64, |scale, product| scale.max(product.abs()));
    if scale == 0.0 {
        return Some(0.0);
    }
    let sum = products.into_iter().sum::<f64>();
    if !sum.is_finite() || sum.abs() / scale < f64::EPSILON {
        return None;
    }
    Some(sum)
}

/// Scale only after splitting the exponent at the finite power-of-two limits.
pub(super) fn scale_power_of_two(value: f64, exponent: i32) -> Option<f64> {
    let outer = exponent.clamp(-1022, 1023);
    let result = (value * 2.0_f64.powi(exponent - outer)) * 2.0_f64.powi(outer);
    result.is_finite().then_some(result)
}

/// The outcome of [`product_sum`].
pub(crate) enum ProductSum {
    /// A term is absent, or a factor is not finite. The sum has no value.
    Undefined,
    /// The terms cancel to exactly zero.
    Zero,
    /// The sum, in the extended exponent range.
    Value(ScaledValue),
}

impl ProductSum {
    /// The two outcomes `ExactSignedSum::finish` and `scaled_finite` state,
    /// whose `None` is an exact zero.
    fn from_scaled(value: Option<ScaledValue>) -> Self {
        match value {
            Some(value) => Self::Value(value),
            None => Self::Zero,
        }
    }
}

/// Sum products without losing a finite result to an intermediate exponent.
/// Ordinary inputs use f64 arithmetic; range loss and cancellation replay the
/// same terms through the exact accumulator.
pub(crate) fn product_sum<const N: usize>(
    terms: impl Iterator<Item = Option<[f64; N]>> + Clone,
) -> ProductSum {
    let mut sum = 0.0_f64;
    let mut largest = 0.0_f64;
    let mut exact = false;
    for factors in terms.clone() {
        let Some(factors) = factors else {
            return ProductSum::Undefined;
        };
        if factors.iter().any(|value| !value.is_finite()) {
            return ProductSum::Undefined;
        }
        if factors.contains(&0.0) {
            continue;
        }
        let mut product = 1.0;
        for factor in factors {
            product *= factor;
            exact |= !product.is_normal();
        }
        sum += product;
        largest = largest.max(product.abs());
        exact |= !sum.is_finite();
    }
    exact |= largest != 0.0 && sum.abs() / largest < f64::EPSILON;
    if !exact {
        return ProductSum::from_scaled(scaled_finite(sum));
    }
    let mut sum = ExactSignedSum::default();
    for factors in terms {
        let Some(factors) = factors else {
            return ProductSum::Undefined;
        };
        sum.add_factors(factors);
    }
    ProductSum::from_scaled(sum.finish())
}

}}
mod ir {use super::*;use cadmpeg_ir::eval::{curve_point_solved,nurbs_curve_parameter_near_point};use crate::math::sum::ExactSignedSum;#[derive(Debug,Clone,Copy)]struct ScalarSweepDifferential{value:f64,derivative:f64}
fn direct_curve_parameter_near_point(
    geometry: &SolvedCurveGeometry,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    if !seed.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let components = |origin: Point3, axis: Vector3, reference: Vector3| -> (f64, f64, f64) {
        let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        let transverse = axis.cross(reference);
        (delta.dot(reference), delta.dot(transverse), delta.dot(axis))
    };
    let parameter = match geometry {
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin();
            let direction = line_curve.direction();
            let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            let denominator = direction.dot(*direction);
            (denominator.is_finite() && denominator > 0.0)
                .then(|| delta.dot(*direction) / denominator)?
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center();
            let axis = circle_curve.axis();
            let ref_direction = circle_curve.ref_direction();
            let radius = circle_curve.radius();
            if radius == 0.0 {
                return None;
            }
            let (x, y, _) = components(*center, *axis, *ref_direction);
            let canonical = (y / radius).atan2(x / radius);
            canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center();
            let axis = ellipse_curve.axis();
            let major_direction = ellipse_curve.major_direction();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            if major_radius == 0.0 || minor_radius == 0.0 {
                return None;
            }
            let (x, y, _) = components(*center, *axis, *major_direction);
            let canonical = (y / minor_radius).atan2(x / major_radius);
            canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        }
        SolvedCurveGeometry::Parabola(parabola_curve) => {
            let vertex = parabola_curve.vertex();
            let axis = parabola_curve.axis();
            let major_direction = parabola_curve.major_direction();
            let focal_distance = parabola_curve.focal_distance();
            if focal_distance == 0.0 {
                return None;
            }
            let (_, transverse, _) = components(*vertex, *axis, *major_direction);
            transverse / (2.0 * focal_distance)
        }
        SolvedCurveGeometry::Hyperbola(hyperbola_curve) => {
            let center = hyperbola_curve.center();
            let axis = hyperbola_curve.axis();
            let major_direction = hyperbola_curve.major_direction();
            let minor_radius = hyperbola_curve.minor_radius();
            if minor_radius == 0.0 {
                return None;
            }
            let (_, transverse, _) = components(*center, *axis, *major_direction);
            (transverse / minor_radius).asinh()
        }
        SolvedCurveGeometry::Nurbs(curve) => {
            nurbs_curve_parameter_near_point(curve, point, tolerance, seed)?
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let (points, parameters) = polyline_samples(polyline);
            polyline_parameter_near_point(&points, &parameters, point, tolerance, seed)?
        }
        SolvedCurveGeometry::Transformed { basis, transform } => {
            let (basis_point, tolerance_scale) = inverse_affine_point(*transform, point)?;
            let basis_tolerance = tolerance * tolerance_scale;
            if !basis_tolerance.is_finite() {
                return None;
            }
            direct_curve_parameter_near_point(basis, basis_point, seed, basis_tolerance)?
        }
        SolvedCurveGeometry::Degenerate(degenerate_curve) => {
            let stored = degenerate_curve.point();
            let error = (stored.x - point.x)
                .hypot(stored.y - point.y)
                .hypot(stored.z - point.z);
            (error.is_finite() && error <= tolerance).then_some(seed)?
        }
        SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. } => return None,
    };
    let evaluated = curve_point_solved(geometry, parameter)?;
    let error = ((evaluated.x - point.x).powi(2)
        + (evaluated.y - point.y).powi(2)
        + (evaluated.z - point.z).powi(2))
    .sqrt();
    (parameter.is_finite() && error.is_finite() && error <= tolerance).then_some(parameter)
}
fn inverse_affine_point(transform: Transform, point: Point3) -> Option<(Point3, f64)> {
    let inverse = transform.try_inverse_affine().ok()?;
    let coordinates = inverse.apply_point(point)?;
    let tolerance_scale = inverse
        .affine_rows()
        .iter()
        .flat_map(|row| &row[..3])
        .fold(0.0_f64, |length, &value| length.hypot(value));
    tolerance_scale
        .is_finite()
        .then_some((coordinates, tolerance_scale))
}
fn polyline_samples(polyline: &PolylineCurve) -> (Vec<Point3>, Vec<f64>) {
    let points: Vec<Point3> = polyline.points().collect();
    let parameters = polyline.parameters().map_or_else(
        || (0..points.len()).map(|index| index as f64).collect(),
        Iterator::collect,
    );
    (points, parameters)
}
fn polyline_parameter_near_point(
    points: &[Point3],
    parameters: &[f64],
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let mut candidates = Vec::new();
    for (segment, parameter_range) in parameters.windows(2).enumerate() {
        let [parameter_start, parameter_end] = [parameter_range[0], parameter_range[1]];
        let parameter_width = parameter_end - parameter_start;
        if !parameter_start.is_finite() || !parameter_end.is_finite() || parameter_width == 0.0 {
            continue;
        }
        let start = points[segment];
        let end = points[segment + 1];
        let direction = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let offset = Vector3::new(point.x - start.x, point.y - start.y, point.z - start.z);
        let length = direction.x.hypot(direction.y).hypot(direction.z);
        if !length.is_finite() {
            continue;
        }
        let fraction = if length == 0.0 {
            if offset.x.hypot(offset.y).hypot(offset.z) > tolerance {
                continue;
            }
            ((seed - parameter_start) / parameter_width).clamp(0.0, 1.0)
        } else {
            let unit = Vector3::new(
                direction.x / length,
                direction.y / length,
                direction.z / length,
            );
            (offset.dot(unit) / length).clamp(0.0, 1.0)
        };
        let candidate = parameter_start + fraction * parameter_width;
        let mapped = Point3::new(
            start.x + fraction * direction.x,
            start.y + fraction * direction.y,
            start.z + fraction * direction.z,
        );
        let error = (mapped.x - point.x)
            .hypot(mapped.y - point.y)
            .hypot(mapped.z - point.z);
        if candidate.is_finite() && error.is_finite() && error <= tolerance {
            candidates.push(candidate);
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| (first - seed).abs().total_cmp(&(second - seed).abs()))
}
fn scalar_unary_sweep_law_differential(
    operator: &str,
    operand: ScalarSweepDifferential,
) -> Option<ScalarSweepDifferential> {
    let x = operand.value;
    match operator {
        "TANH" => {
            let exponential = (-x.abs()).exp();
            let mut numerator = ExactSignedSum::default();
            numerator.add_factors([4.0, exponential, exponential, operand.derivative]);
            let denominator =
                crate::math::sum::scaled_finite((1.0 + exponential * exponential).powi(2))?;
            let derivative = match numerator.finish() {
                Some(value) => value.quotient(denominator)?,
                None => 0.0,
            };
            return finite_sweep_differential(x.tanh(), derivative);
        }
        "ARCSINH" => {
            return finite_sweep_differential(x.asinh(), operand.derivative / x.hypot(1.0))
        }
        "ARCCOSH" => {
            if x <= 1.0 {
                return None;
            }
            let denominator = if x < 2.0 {
                ((x - 1.0) * (x + 1.0)).sqrt()
            } else {
                x * (1.0 - (1.0 / x).powi(2)).sqrt()
            };
            return finite_sweep_differential(x.acosh(), operand.derivative / denominator);
        }
        "ARCOTH" => {
            if x.abs() <= 1.0 {
                return None;
            }
            let inverse = 1.0 / x;
            let denominator = ((x - 1.0) / x) * ((x + 1.0) / x);
            let derivative =
                crate::math::multiply_divide(operand.derivative / x, -inverse, denominator)?;
            return finite_sweep_differential(inverse.atanh(), derivative);
        }
        _ => {}
    }

    let derivative = match operator {
        "SIN" => x.cos(),
        "COS" => -x.sin(),
        "TAN" => {
            let cosine = x.cos();
            (cosine != 0.0).then_some(1.0 / (cosine * cosine))?
        }
        "COT" => {
            let sine = x.sin();
            (sine != 0.0).then_some(-1.0 / (sine * sine))?
        }
        "SEC" => {
            let cosine = x.cos();
            (cosine != 0.0).then_some(1.0 / cosine * x.tan())?
        }
        "CSC" => {
            let sine = x.sin();
            (sine != 0.0).then_some(-(1.0 / sine) * (x.cos() / sine))?
        }
        "COSH" => x.sinh(),
        "SINH" => x.cosh(),
        "COTH" => {
            let sinh = x.sinh();
            (sinh != 0.0).then_some(-1.0 / (sinh * sinh))?
        }
        "SECH" => {
            let value = 1.0 / x.cosh();
            -value * x.tanh()
        }
        "CSCH" => {
            let sinh = x.sinh();
            (sinh != 0.0).then_some(-(1.0 / sinh) * (x.cosh() / sinh))?
        }
        "ARCCOS" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(-1.0 / denominator)?
        }
        "ARCSIN" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCTAN" => 1.0 / (1.0 + x * x),
        "ARCOT" => -1.0 / (1.0 + x * x),
        "ARCSEC" => {
            let denominator = (x * x - 1.0).sqrt();
            (x.abs() > 1.0 && denominator > 0.0).then_some(1.0 / (x.abs() * denominator))?
        }
        "ARCCSC" => {
            let denominator = (x * x - 1.0).sqrt();
            (x.abs() > 1.0 && denominator > 0.0).then_some(-1.0 / (x.abs() * denominator))?
        }
        "ARCTANH" => (x.abs() < 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ARCSECH" => {
            let denominator = (1.0 - x * x).sqrt();
            (x > 0.0 && x < 1.0 && denominator > 0.0).then_some(-1.0 / (x * denominator))?
        }
        "ARCCSCH" => (x != 0.0).then_some(-1.0 / (x.abs() * (1.0 + x * x).sqrt()))?,
        "ABS" => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                return None;
            }
        }
        "EXP" => x.exp(),
        "LN" => (x > 0.0).then_some(1.0 / x)?,
        "SIGN" => (x != 0.0).then_some(0.0)?,
        "SQRT" => (x > 0.0).then_some(0.5 / x.sqrt())?,
        _ => return None,
    };
    finite_sweep_differential(
        match operator {
            "SIN" => x.sin(),
            "COS" => x.cos(),
            "TAN" => x.tan(),
            "COT" => 1.0 / x.tan(),
            "SEC" => 1.0 / x.cos(),
            "CSC" => 1.0 / x.sin(),
            "COSH" => x.cosh(),
            "SINH" => x.sinh(),
            "COTH" => 1.0 / x.tanh(),
            "SECH" => 1.0 / x.cosh(),
            "CSCH" => 1.0 / x.sinh(),
            "ARCCOS" => x.acos(),
            "ARCSIN" => x.asin(),
            "ARCTAN" => x.atan(),
            "ARCOT" => std::f64::consts::FRAC_PI_2 - x.atan(),
            "ARCSEC" => (1.0 / x).acos(),
            "ARCCSC" => (1.0 / x).asin(),
            "ARCTANH" => x.atanh(),
            "ARCSECH" => (1.0 / x).acosh(),
            "ARCCSCH" => (1.0 / x).asinh(),
            "ABS" => x.abs(),
            "EXP" => x.exp(),
            "LN" => x.ln(),
            "SIGN" => x.signum(),
            "SQRT" => x.sqrt(),
            _ => return None,
        },
        derivative * operand.derivative,
    )
}
fn finite_sweep_differential(value: f64, derivative: f64) -> Option<ScalarSweepDifferential> {
    (value.is_finite() && derivative.is_finite())
        .then_some(ScalarSweepDifferential { value, derivative })
}

#[test]fn line_false_witness(){let g=SolvedCurveGeometry::Line(cadmpeg_ir::geometry::analytic::LineCurve::try_new(Point3::new(0.,0.,0.),Vector3::new(1.,0.,0.)).unwrap());let got=direct_curve_parameter_near_point(&g,Point3::new(0.,1e-200,0.),0.,0.);println!("IR analytic line: returned={got:?}; actual residual=1e-200; tolerance=0");assert_eq!(got,Some(0.));}
#[test]fn remaining_unary_laws(){for op in ["ARCTAN","ARCOT","ARCSEC","ARCCSC","ARCCSCH"]{let got=scalar_unary_sweep_law_differential(op,ScalarSweepDifferential{value:1e200,derivative:1e300}).unwrap();println!("IR {op}: derivative={} expected magnitude~1e-100",got.derivative);assert_eq!(got.derivative,0.);}let got=scalar_unary_sweep_law_differential("COTH",ScalarSweepDifferential{value:400.,derivative:1e300}).unwrap();println!("IR COTH(400): derivative={} expected~-1.47e-47",got.derivative);assert_eq!(got.derivative,0.);for op in ["SECH","CSCH"]{let got=scalar_unary_sweep_law_differential(op,ScalarSweepDifferential{value:720.,derivative:1e300});println!("IR {op}(720): {got:?}; expected value~4.064e-313 derivative~-4.064e-13");assert!(got.is_none_or(|d|d.derivative==0.));}}
}
mod f3d {use super::*;enum ProfileBoundarySegment{Line{start:Point2,end:Point2},Arc{center:Point2,radius:f64,start_angle:f64,end_angle:f64}}
fn segments_intersect(left: (Point2, Point2), right: (Point2, Point2)) -> bool {
    fn side(line: (Point2, Point2), point: Point2) -> f64 {
        (line.1.u - line.0.u) * (point.v - line.0.v) - (line.1.v - line.0.v) * (point.u - line.0.u)
    }

    let left_start = side(left, right.0);
    let left_end = side(left, right.1);
    let right_start = side(right, left.0);
    let right_end = side(right, left.1);
    if left_start == 0.0 && left_end == 0.0 && right_start == 0.0 && right_end == 0.0 {
        let overlaps = |a0: f64, a1: f64, b0: f64, b1: f64| {
            a0.min(a1) <= b0.max(b1) && b0.min(b1) <= a0.max(a1)
        };
        return overlaps(left.0.u, left.1.u, right.0.u, right.1.u)
            && overlaps(left.0.v, left.1.v, right.0.v, right.1.v);
    }
    left_start * left_end <= 0.0 && right_start * right_end <= 0.0
}
fn arc_intersection_points(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<Vec<Point2>> {
    let ProfileBoundarySegment::Arc {
        center: lc,
        radius: lr,
        start_angle: ls,
        end_angle: le,
    } = left
    else {
        return None;
    };
    let ProfileBoundarySegment::Arc {
        center: rc,
        radius: rr,
        start_angle: rs,
        end_angle: re,
    } = right
    else {
        return None;
    };
    let du = rc.u - lc.u;
    let dv = rc.v - lc.v;
    let distance_squared = du * du + dv * dv;
    if distance_squared == 0.0 {
        return (*lr != *rr).then(Vec::new);
    }
    let distance = distance_squared.sqrt();
    if distance > lr + rr || distance < (lr - rr).abs() {
        return Some(Vec::new());
    }
    let along = (lr * lr - rr * rr + distance_squared) / (2.0 * distance);
    let height_squared = lr * lr - along * along;
    let error = 64.0 * f64::EPSILON * (lr * lr + along * along).max(1.0);
    if height_squared < -error {
        return Some(Vec::new());
    }
    let base = Point2::new(lc.u + along * du / distance, lc.v + along * dv / distance);
    let height = height_squared.max(0.0).sqrt();
    let mut points = Vec::new();
    for signed_height in [height, -height] {
        let point = Point2::new(
            base.u - signed_height * dv / distance,
            base.v + signed_height * du / distance,
        );
        if directed_angle_parameter((point.v - lc.v).atan2(point.u - lc.u), *ls, *le).is_some()
            && directed_angle_parameter((point.v - rc.v).atan2(point.u - rc.u), *rs, *re).is_some()
            && !points.contains(&point)
        {
            points.push(point);
        }
    }
    Some(points)
}
fn directed_angle_parameter(angle: f64, start: f64, end: f64) -> Option<f64> {
    let sweep = end - start;
    if sweep == 0.0 || sweep.abs() > std::f64::consts::TAU {
        return None;
    }
    let displacement = if sweep > 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        -(start - angle).rem_euclid(std::f64::consts::TAU)
    };
    (displacement.abs() <= sweep.abs()).then_some(displacement / sweep)
}
fn point_distance(a: Point2, b: Point2) -> f64 {
    ((a.u - b.u).powi(2) + (a.v - b.v).powi(2)).sqrt()
}

#[test]fn separated_segments(){let a=1e-100;let got=segments_intersect((Point2::new(0.,0.),Point2::new(a,0.)),(Point2::new(0.,2.*a),Point2::new(a,2.*a)));println!("F3D separated parallel segments intersect={got}; separation={:e}",2.*a);assert!(got);}
#[test]fn crossing_circles(){for r in [1.,1e-200,1e200]{let arc=|x|ProfileBoundarySegment::Arc{center:Point2::new(x,0.),radius:r,start_angle:0.,end_angle:std::f64::consts::TAU};let got=arc_intersection_points(&arc(0.),&arc(r));println!("F3D crossing circles radius={r:e}: {got:?}; expected 2 points");if r==1.{assert_eq!(got.unwrap().len(),2)}else{assert!(got.is_none_or(|p|p.len()!=2));}}}
#[test]fn distance_range(){for a in [1e-200,1e200]{let got=point_distance(Point2::new(0.,0.),Point2::new(a,0.));println!("F3D distance={got:e}, expected={a:e}");assert_ne!(got,a);}}
}
mod creo {use super::*;const EPS_RADIUS_NONZERO:f64=1e-12;const EPS_RADIAL_RESIDUAL:f64=1e-10;const EPS_PARAMETER_BOUND:f64=1e-10;const EPS_CENTER_DISTANCE:f64=1e-12;const EPS_HEIGHT_RESIDUAL:f64=1e-9;const EPS_LINE_INTERSECTION:f64=1e-12;fn intersect_section_line_arc(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        (line @ SketchGeometryDefinition::Line { .. }, arc @ SketchGeometryDefinition::Arc { .. })
        | (arc @ SketchGeometryDefinition::Arc { .. }, line @ SketchGeometryDefinition::Line { .. }),
    ) = ((first.definition(), second.definition()),)
    else {
        return None;
    };
    let SketchGeometryDefinition::Line { start, end } = line else {
        return None;
    };
    let SketchGeometryDefinition::Arc { center, radius, .. } = arc else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    if length <= EPS_RADIUS_NONZERO || radius.get() <= EPS_RADIUS_NONZERO {
        return None;
    }
    let direction = direction.map(|value| value / length);
    let relative = [start.u - center.u, start.v - center.v];
    let projection = -(relative[0] * direction[0] + relative[1] * direction[1]);
    let closest = [
        start.u + projection * direction[0],
        start.v + projection * direction[1],
    ];
    let distance_squared = (closest[0] - center.u).mul_add(
        closest[0] - center.u,
        (closest[1] - center.v) * (closest[1] - center.v),
    );
    let radial_squared = radius.get() * radius.get();
    let scale = radial_squared.max(1.0);
    if distance_squared > radial_squared + EPS_RADIAL_RESIDUAL * scale {
        return None;
    }
    let travel = (radial_squared - distance_squared).max(0.0).sqrt();
    let candidates = [
        [
            closest[0] + travel * direction[0],
            closest[1] + travel * direction[1],
        ],
        [
            closest[0] - travel * direction[0],
            closest[1] - travel * direction[1],
        ],
    ];
    if travel <= EPS_RADIAL_RESIDUAL * radius.get().max(1.0) {
        let parameter = projection / length;
        return (-EPS_PARAMETER_BOUND..=1.0 + EPS_PARAMETER_BOUND)
            .contains(&parameter)
            .then_some(candidates[0]);
    }
    let parameters = [
        (projection + travel) / length,
        (projection - travel) / length,
    ];
    let inside = parameters
        .into_iter()
        .enumerate()
        .filter(|(_, parameter)| {
            (-EPS_PARAMETER_BOUND..=1.0 + EPS_PARAMETER_BOUND).contains(parameter)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = inside.as_slice() else {
        return None;
    };
    Some(candidates[*index])
}
fn intersect_tangent_section_arcs(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        SketchGeometryDefinition::Arc {
            center: first_center,
            radius: first_radius,
            ..
        },
        SketchGeometryDefinition::Arc {
            center: second_center,
            radius: second_radius,
            ..
        },
    ) = (first.definition(), second.definition())
    else {
        return None;
    };
    if first_radius.get() <= EPS_RADIUS_NONZERO || second_radius.get() <= EPS_RADIUS_NONZERO {
        return None;
    }
    let delta = [
        second_center.u - first_center.u,
        second_center.v - first_center.v,
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance
        .max(first_radius.get())
        .max(second_radius.get())
        .max(1.0);
    if distance <= EPS_CENTER_DISTANCE * scale {
        return None;
    }
    let offset = (first_radius.get().mul_add(
        first_radius.get(),
        -(second_radius.get() * second_radius.get()),
    ) + distance * distance)
        / (2.0 * distance);
    let height_squared = first_radius
        .get()
        .mul_add(first_radius.get(), -(offset * offset));
    if height_squared.abs() > EPS_HEIGHT_RESIDUAL * scale * scale {
        return None;
    }
    Some([
        first_center.u + offset * delta[0] / distance,
        first_center.v + offset * delta[1] / distance,
    ])
}
fn section_line_origin_direction(geometry: &SketchGeometry) -> Option<(Point2, Point2)> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            Some((*start, Point2::new(end.u - start.u, end.v - start.v)))
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            Some((*origin, *direction))
        }
        _ => None,
    }
}
fn intersect_section_lines(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (first_origin, first_direction) = section_line_origin_direction(first)?;
    let (second_origin, second_direction) = section_line_origin_direction(second)?;
    let first_end = Point2::new(
        first_origin.u + first_direction.u,
        first_origin.v + first_direction.v,
    );
    let second_end = Point2::new(
        second_origin.u + second_direction.u,
        second_origin.v + second_direction.v,
    );
    let denominator = (first_origin.u - first_end.u).mul_add(
        second_origin.v - second_end.v,
        -(first_origin.v - first_end.v) * (second_origin.u - second_end.u),
    );
    let scale = (first_origin.u - first_end.u)
        .abs()
        .max((first_origin.v - first_end.v).abs())
        .max((second_origin.u - second_end.u).abs())
        .max((second_origin.v - second_end.v).abs())
        .max(1.0);
    if denominator.abs() <= EPS_LINE_INTERSECTION * scale * scale {
        return None;
    }
    let first_cross = first_origin
        .u
        .mul_add(first_end.v, -(first_origin.v * first_end.u));
    let second_cross = second_origin
        .u
        .mul_add(second_end.v, -(second_origin.v * second_end.u));
    Some([
        first_cross.mul_add(
            second_origin.u - second_end.u,
            -(first_origin.u - first_end.u) * second_cross,
        ) / denominator,
        first_cross.mul_add(
            second_origin.v - second_end.v,
            -(first_origin.v - first_end.v) * second_cross,
        ) / denominator,
    ])
}

#[test]fn missed_circle(){let r=1e-6;let got=intersect_section_line_arc(&line([-r,2.*r],[r,2.*r]),&arc([0.,0.],r));println!("Creo missed circle: {got:?}; closest distance=2e-6; radius=1e-6");assert_eq!(got,Some([0.,2.*r]));}
#[test]fn nontangent_circles(){let r=1e-6;let got=intersect_tangent_section_arcs(&arc([0.,0.],r),&arc([r,0.],r));println!("Creo two crossing circles claimed tangent={got:?}; true intersections=(0.5e-6,+/-sqrt(3)*0.5e-6)");assert_eq!(got,Some([r*0.5,0.]));}
#[test]fn small_perpendicular_lines(){let r=1e-7;let got=intersect_section_lines(&line([-r,0.],[r,0.]),&line([0.,-r],[0.,r]));println!("Creo perpendicular lines crossing origin: {got:?}; expected Some([0,0])");assert!(got.is_none());}
}
mod catia {use super::*;const NURBS_SURFACE_MEMBERSHIP_TOLERANCE:f64=2e-3;const NURBS_SURFACE_SEEDS_PER_SPAN:usize=3;const NURBS_SURFACE_MAX_SEEDS:usize=256;const NURBS_SURFACE_REFINEMENT_ITERATIONS:usize=24;const NURBS_SURFACE_BACKTRACK_STEPS:usize=8;fn nurbs_surface_parameter_domain(surface: &NurbsSurface) -> Option<[[f64; 2]; 2]> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let domains = [
        [
            *surface.u_knots().get(u_degree)?,
            *surface.u_knots().get(u_count)?,
        ],
        [
            *surface.v_knots().get(v_degree)?,
            *surface.v_knots().get(v_count)?,
        ],
    ];
    domains
        .into_iter()
        .all(|[lower, upper]| lower.is_finite() && upper.is_finite() && lower < upper)
        .then_some(domains)
}
fn refine_nurbs_surface_point(
    surface: &NurbsSurface,
    point: Point3,
    seed: Point2,
    domains: [[f64; 2]; 2],
) -> Option<f64> {
    let mut parameters = seed;
    for _ in 0..NURBS_SURFACE_REFINEMENT_ITERATIONS {
        let partials =
            cadmpeg_ir::eval::nurbs_surface_partials(surface, parameters.u, parameters.v)?;
        let residual = partials.point.vector_from(point);
        let du_squared = partials.du.dot(partials.du);
        let mixed = partials.du.dot(partials.dv);
        let dv_squared = partials.dv.dot(partials.dv);
        let determinant = du_squared * dv_squared - mixed * mixed;
        if !determinant.is_finite()
            || determinant.abs() <= f64::EPSILON * du_squared.max(dv_squared).powi(2)
        {
            break;
        }
        let du_residual = partials.du.dot(residual);
        let dv_residual = partials.dv.dot(residual);
        let step = Point2::new(
            (dv_squared * du_residual - mixed * dv_residual) / determinant,
            (du_squared * dv_residual - mixed * du_residual) / determinant,
        );
        let current = nurbs_surface_point_distance_squared(surface, point, parameters)?;
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..NURBS_SURFACE_BACKTRACK_STEPS {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(domains[0][0], domains[0][1]),
                (parameters.v - scale * step.v).clamp(domains[1][0], domains[1][1]),
            );
            let distance = nurbs_surface_point_distance_squared(surface, point, candidate)?;
            if distance <= current {
                accepted = Some((candidate, distance));
                break;
            }
            scale *= 0.5;
        }
        let Some((candidate, distance)) = accepted else {
            break;
        };
        parameters = candidate;
        if distance <= NURBS_SURFACE_MEMBERSHIP_TOLERANCE.powi(2) {
            return Some(distance);
        }
    }
    nurbs_surface_point_distance_squared(surface, point, parameters)
}
fn nurbs_surface_point_distance_squared(
    surface: &NurbsSurface,
    point: Point3,
    uv: Point2,
) -> Option<f64> {
    let position = cadmpeg_ir::eval::nurbs_surface_point(surface, uv.u, uv.v)?;
    let distance = position.vector_from(point);
    let squared = distance.dot(distance);
    squared.is_finite().then_some(squared)
}
fn nurbs_surface_witness_distance(surface: &NurbsSurface, point: Point3) -> Option<f64> {
    let domains = nurbs_surface_parameter_domain(surface)?;
    let starts = nurbs_surface_start_grid(surface, domains)?;
    starts
        .into_iter()
        .filter_map(|seed| refine_nurbs_surface_point(surface, point, seed, domains))
        .min_by(f64::total_cmp)
}
fn nurbs_surface_axis_samples(knots: &[f64], degree: usize, count: usize) -> Option<Vec<f64>> {
    let mut boundaries = Vec::new();
    for &knot in knots.get(degree..=count)? {
        if boundaries.last().is_none_or(|previous| *previous != knot) {
            boundaries.push(knot);
        }
    }
    let mut samples = Vec::new();
    for pair in boundaries.windows(2) {
        let [lower, upper] = *pair else {
            continue;
        };
        if !lower.is_finite() || !upper.is_finite() || lower >= upper {
            continue;
        }
        for step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
            let fraction = step as f64 / (NURBS_SURFACE_SEEDS_PER_SPAN - 1) as f64;
            samples.push(lower + fraction * (upper - lower));
        }
    }
    (!samples.is_empty()).then_some(samples)
}
fn nurbs_surface_start_grid(surface: &NurbsSurface, domains: [[f64; 2]; 2]) -> Option<Vec<Point2>> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_samples = nurbs_surface_axis_samples(surface.u_knots(), u_degree, u_count)?;
    let v_samples = nurbs_surface_axis_samples(surface.v_knots(), v_degree, v_count)?;
    if u_samples.len().checked_mul(v_samples.len())? > NURBS_SURFACE_MAX_SEEDS {
        let side = (NURBS_SURFACE_MAX_SEEDS as f64).sqrt() as usize;
        let mut grid = Vec::with_capacity(side * side);
        for u in 0..side {
            for v in 0..side {
                let u_fraction = u as f64 / (side - 1) as f64;
                let v_fraction = v as f64 / (side - 1) as f64;
                grid.push(Point2::new(
                    domains[0][0] + u_fraction * (domains[0][1] - domains[0][0]),
                    domains[1][0] + v_fraction * (domains[1][1] - domains[1][0]),
                ));
            }
        }
        return Some(grid);
    }
    Some(
        u_samples
            .into_iter()
            .flat_map(|u| v_samples.iter().copied().map(move |v| Point2::new(u, v)))
            .collect(),
    )
}

#[test]fn anisotropic_surface(){let u=NurbsSurfaceAxis::new(1,vec![0.,0.,1e-10,1e-10],false);let v=NurbsSurfaceAxis::new(1,vec![0.,0.,1.,1.],false);let sf=NurbsSurface::from_lanes(u,v,NurbsSurfaceLanes::new(vec![vec![Point3::new(0.,0.,0.),Point3::new(0.,1.,0.)],vec![Point3::new(1.,0.,0.),Point3::new(1.,1.,0.)]],None),false).unwrap();let p=Point3::new(0.3,0.4,0.);let got=nurbs_surface_witness_distance(&sf,p).unwrap();let exact=cadmpeg_ir::eval::nurbs_surface_point(&sf,3e-11,0.4).unwrap();println!("CATIA point on plane: best squared residual={got:e}; known UV=(3e-11,0.4); exact point={exact:?}");assert!(got>0.01);assert!(exact.distance(p)<NURBS_SURFACE_MEMBERSHIP_TOLERANCE);}}
mod nx {use super::*;fn determinant_3x3(matrix: [[f64; 3]; 3]) -> f64 {
    matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
}
fn null_vector_3x4(matrix: [[f64; 4]; 3]) -> Option<[f64; 4]> {
    let mut vector = [0.0; 4];
    for (omitted, component) in vector.iter_mut().enumerate() {
        let minor = std::array::from_fn(|row| {
            let mut column = 0;
            std::array::from_fn(|_| {
                while column == omitted {
                    column += 1;
                }
                let value = matrix[row][column];
                column += 1;
                value
            })
        });
        *component = if omitted % 2 == 0 { 1.0 } else { -1.0 } * determinant_3x3(minor);
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    (norm.is_finite() && norm > 1.0e-14).then(|| vector.map(|value| value / norm))
}
fn lift_periodic_parameter(value: f64, reference: f64, period: f64) -> f64 {
    value + ((reference - value) / period).round() * period
}

#[test]fn tangent_scale(){for a in [1.,1e-5,1e100]{let got=null_vector_3x4([[a,0.,-a,0.],[0.,a,0.,0.],[0.,0.,0.,-a]]);println!("NX rank-three matrix scale={a:e}: nullvector={got:?}; expected a normalized (1,0,1,0)");assert_eq!(got.is_some(),a==1.);}}
#[test]fn finite_phase(){let got=lift_periodic_parameter(-1e308,1e308,1e307);println!("NX periodic lift={got}; finite congruent nearest representative is 1e308");assert!(got.is_infinite());}}
mod sld {use super::*;const SKETCH_POINT_TOLERANCE:f64=1e-9;fn quantize(point: Point2, quantum: f64) -> (i64, i64) {
    (
        (point.u / quantum).round() as i64,
        (point.v / quantum).round() as i64,
    )
}
fn line_line_distance(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> Option<f64> {
    let first_direction = line_direction(first);
    let second_direction = line_direction(second);
    let first_length = first_direction[0].hypot(first_direction[1]);
    let second_length = second_direction[0].hypot(second_direction[1]);
    if first_length <= SKETCH_POINT_TOLERANCE || second_length <= SKETCH_POINT_TOLERANCE {
        return None;
    }
    let cross = |left: [f64; 2], right: [f64; 2]| left[0] * right[1] - left[1] * right[0];
    if cross(first_direction, second_direction).abs()
        > SKETCH_POINT_TOLERANCE * first_length * second_length
    {
        return None;
    }
    Some(
        cross(
            [second[0][0] - first[0][0], second[0][1] - first[0][1]],
            first_direction,
        )
        .abs()
            / first_length,
    )
}
fn line_direction(line: [[f64; 2]; 2]) -> [f64; 2] {
    [line[1][0] - line[0][0], line[1][1] - line[0][1]]
}

#[test]fn saturated_keys(){let a=quantize(Point2::new(1e14,0.),1e-6);let b=quantize(Point2::new(2e14,0.),1e-6);println!("SLD separated points 1e14 and 2e14 quantum=1e-6: keys={a:?},{b:?}");assert_eq!(a,b);}
#[test]fn nonparallel_distance(){let got=line_line_distance([[0.,0.],[1e200,0.]],[[0.,1.],[0.,1e200]]);println!("SLD perpendicular lines admitted as parallel: distance={got:?}; expected None");assert_eq!(got,Some(1.));}
}
mod iges {use super::*;fn distance(left: Point3, right: Point3) -> f64 {
    ((left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)).sqrt()
}

#[test]fn residual_range(){for a in [1e-200,1e200]{let got=distance(Point3::new(0.,0.,0.),Point3::new(a,0.,0.));println!("iges distance={got:e}, expected={a:e}");assert_ne!(got,a);}}}
mod nx_distance {use super::*;fn distance(first: Point3, second: Point3) -> f64 {
    ((first.x - second.x).powi(2) + (first.y - second.y).powi(2) + (first.z - second.z).powi(2))
        .sqrt()
}

#[test]fn residual_range(){for a in [1e-200,1e200]{let got=distance(Point3::new(0.,0.,0.),Point3::new(a,0.,0.));println!("nx_distance distance={got:e}, expected={a:e}");assert_ne!(got,a);}}}
mod inventor {use super::*;const EPS_SKETCH_LINE_CARRIER_MATCHES_E10:f64=1e-10;fn line_carrier_matches(
    origin: [f64; 2],
    direction: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> bool {
    let norm = direction[0].hypot(direction[1]);
    let span = [end[0] - start[0], end[1] - start[1]];
    let span_norm = span[0].hypot(span[1]);
    if norm <= f64::EPSILON || span_norm <= f64::EPSILON {
        return false;
    }
    let scale = norm * span_norm;
    let parallel_error = (direction[0] * span[1] - direction[1] * span[0]).abs() / scale;
    let from_origin = [start[0] - origin[0], start[1] - origin[1]];
    let origin_scale = norm * from_origin[0].hypot(from_origin[1]).max(1.0);
    let carrier_error =
        (direction[0] * from_origin[1] - direction[1] * from_origin[0]).abs() / origin_scale;
    parallel_error <= EPS_SKETCH_LINE_CARRIER_MATCHES_E10
        && carrier_error <= EPS_SKETCH_LINE_CARRIER_MATCHES_E10
}

#[test]fn finite_parallel(){let got=line_carrier_matches([0.,0.],[1e200,1e200],[1e200,1e200],[2e200,2e200]);println!("Inventor exact collinear finite carrier accepted={got}");assert!(!got);}}
mod step {use super::*;use cadmpeg_ir::transform::Transform2;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10:f64=1e-10;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12:f64=1e-12;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10:f64=1e-10;const EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12:f64=1e-12;fn similarity_transform(transform: &Transform) -> bool {
    if transform
        .rows()
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
        || transform.rows()[3][0].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || transform.rows()[3][1].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || transform.rows()[3][2].abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        || (transform.rows()[3][3] - 1.0).abs() > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
    {
        return false;
    }
    let columns = [
        Vector3::new(
            transform.rows()[0][0],
            transform.rows()[1][0],
            transform.rows()[2][0],
        ),
        Vector3::new(
            transform.rows()[0][1],
            transform.rows()[1][1],
            transform.rows()[2][1],
        ),
        Vector3::new(
            transform.rows()[0][2],
            transform.rows()[1][2],
            transform.rows()[2][2],
        ),
    ];
    let scale = columns[0].norm();
    let tolerance = EPS_GEOMETRY_SIMILARITY_TRANSFORM_E10 * scale.max(1.0);
    scale > EPS_GEOMETRY_SIMILARITY_TRANSFORM_E12
        && columns
            .iter()
            .all(|column| (column.norm() - scale).abs() <= tolerance)
        && columns[0].dot(columns[1]).abs() <= tolerance * scale
        && columns[0].dot(columns[2]).abs() <= tolerance * scale
        && columns[1].dot(columns[2]).abs() <= tolerance * scale
}
fn similarity_transform_2d(transform: &Transform2) -> bool {
    let first = Point2::new(transform.rows()[0][0], transform.rows()[1][0]);
    let second = Point2::new(transform.rows()[0][1], transform.rows()[1][1]);
    let scale = first.u.hypot(first.v);
    let tolerance = EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E10 * scale.max(1.0);
    scale > EPS_GEOMETRY_SIMILARITY_TRANSFORM_2D_E12
        && (second.u.hypot(second.v) - scale).abs() <= tolerance
        && (first.u * second.u + first.v * second.v).abs() <= tolerance * scale
}

#[test]fn small_shear(){let a=1e-10;let t=Transform::affine([[a,0.5*a,0.,0.],[0.,0.75f64.sqrt()*a,0.,0.],[0.,0.,a,0.]]).unwrap();let t2=Transform2::affine([[a,0.5*a,0.],[0.,0.75f64.sqrt()*a,0.]]).unwrap();let got=similarity_transform(&t);let got2=similarity_transform_2d(&t2);println!("STEP 60-degree columns accepted as similarity: 3d={got}, 2d={got2}");assert!(got&&got2);}}
mod rhino {use super::*;fn periodic_knots(knots: &[f64], order: usize, cv_count: usize) -> bool {
    // This is ON_IsKnotVectorPeriodic over the stored, zero-based knot array.
    if order < 3 || cv_count < order || (order <= 4 && cv_count < order + 2) {
        return false;
    }
    if order > 4 && cv_count < 2 * order - 2 {
        return false;
    }
    let mut tolerance = (knots[order - 1] - knots[order - 3]).abs() * f64::EPSILON.sqrt();
    tolerance = tolerance.max((knots[cv_count - 1] - knots[order - 2]).abs() * f64::EPSILON.sqrt());
    let mut paired = 2 * (order - 2);
    let mut index = 0;
    let mut other = cv_count - order + 1;
    while paired > 0 {
        if ((knots[index + 1] - knots[index]) + (knots[other] - knots[other + 1])).abs() > tolerance
        {
            return false;
        }
        index += 1;
        other += 1;
        paired -= 1;
    }
    true
}
fn map_parameter(value: f64, domain: [f64; 2], extents: [f64; 2]) -> f64 {
    extents[0] + (value - domain[0]) * (extents[1] - extents[0]) / (domain[1] - domain[0])
}

#[test]fn plane_parameter_range(){for a in [1e-200,1e200]{let got=map_parameter(a,[0.,a],[0.,a]);println!("Rhino identity plane parameter map: got={got:e}, expected={a:e}");assert_ne!(got,a);}}
#[test]fn nonperiodic_knots(){let k=[-1e308,-1e308,-9e307,9e307,1e308,1e308];let got=periodic_knots(&k,3,5);println!("Rhino nonperiodic knots admitted periodic={got}; paired gaps 0 and 1e307 differ");assert!(got);}}
mod creo_profile {use super::*;fn point_on_profile_arc(
    point: [f64; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let (center, radius, start, delta) = arc;
    let relative = [point[0] - center[0], point[1] - center[1]];
    let distance = relative[0].hypot(relative[1]);
    if (distance - radius).abs() > tolerance {
        return false;
    }
    let angle = relative[1].atan2(relative[0]);
    let travel = if delta >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    travel <= delta.abs() + tolerance / radius.max(1.0)
}
fn line_arc_intersect(
    line: [[f64; 2]; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let direction = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
    let relative = [line[0][0] - arc.0[0], line[0][1] - arc.0[1]];
    let a = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    let b = 2.0 * direction[0].mul_add(relative[0], direction[1] * relative[1]);
    let c = relative[0].mul_add(relative[0], relative[1] * relative[1]) - arc.1 * arc.1;
    let discriminant = b.mul_add(b, -(4.0 * a * c));
    if a <= tolerance * tolerance || discriminant < -tolerance * tolerance {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-b + signed_root) / (2.0 * a);
        parameter >= -tolerance
            && parameter <= 1.0 + tolerance
            && point_on_profile_arc(
                [
                    line[0][0] + parameter * direction[0],
                    line[0][1] + parameter * direction[1],
                ],
                arc,
                tolerance,
            )
    })
}
fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
                [
                    arc.0[0] + arc.1 * arc.2.cos(),
                    arc.0[1] + arc.1 * arc.2.sin(),
                ],
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3).sin(),
                ],
            ]
        };
        return endpoints(first)
            .into_iter()
            .any(|point| point_on_profile_arc(point, second, tolerance))
            || endpoints(second)
                .into_iter()
                .any(|point| point_on_profile_arc(point, first, tolerance));
    }
    if distance <= tolerance
        || distance > first.1 + second.1 + tolerance
        || distance < (first.1 - second.1).abs() - tolerance
    {
        return false;
    }
    let along = (first.1 * first.1 - second.1 * second.1 + distance * distance) / (2.0 * distance);
    let height_squared = first.1 * first.1 - along * along;
    if height_squared < -tolerance * tolerance {
        return false;
    }
    let base = [
        first.0[0] + along * displacement[0] / distance,
        first.0[1] + along * displacement[1] / distance,
    ];
    let height = height_squared.max(0.0).sqrt();
    let offset = [
        -height * displacement[1] / distance,
        height * displacement[0] / distance,
    ];
    [-1.0, 1.0].into_iter().any(|sign| {
        let point = [base[0] + sign * offset[0], base[1] + sign * offset[1]];
        point_on_profile_arc(point, first, tolerance)
            && point_on_profile_arc(point, second, tolerance)
    })
}

#[test]fn large_intersections(){let a=1e200;let circle=([0.,0.],a,0.,std::f64::consts::TAU);let line_hit=line_arc_intersect([[-2.*a,0.],[2.*a,0.]],circle,1e-9);let circle_hit=arcs_intersect(circle,([a,0.],a,0.,std::f64::consts::TAU),1e-9);println!("Creo profile: line-circle={line_hit}, circle-circle={circle_hit}; both have two intersections");assert!(!line_hit&&!circle_hit);}}
mod sld_ellipse {use super::*;const EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9:f64=1e-9;struct SketchEntity{geometry:SketchGeometry}const SKETCH_POINT_TOLERANCE:f64=1e-9;const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12:f64=1e-12;const EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9:f64=1e-9;fn sketch_entity_contains_point(entity: &SketchEntity, point: Point2) -> bool {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let du = end.u - start.u;
            let dv = end.v - start.v;
            let length_squared = du * du + dv * dv;
            if length_squared <= SKETCH_POINT_TOLERANCE * SKETCH_POINT_TOLERANCE {
                return false;
            }
            let parameter = ((point.u - start.u) * du + (point.v - start.v) * dv) / length_squared;
            let distance =
                ((point.u - start.u) * dv - (point.v - start.v) * du).abs() / length_squared.sqrt();
            distance <= SKETCH_POINT_TOLERANCE
                && (-SKETCH_POINT_TOLERANCE..=1.0 + SKETCH_POINT_TOLERANCE).contains(&parameter)
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            length > SKETCH_POINT_TOLERANCE
                && ((point.u - origin.u) * direction.v - (point.v - origin.v) * direction.u).abs()
                    <= SKETCH_POINT_TOLERANCE * length
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.get())
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            if !same_dimension_length((point.u - center.u).hypot(point.v - center.v), radius.get())
            {
                return false;
            }
            let raw = end_angle.get() - start_angle.get();
            let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
            if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
            {
                sweep = std::f64::consts::TAU;
            }
            let parameter = ((point.v - center.v).atan2(point.u - center.u) - start_angle.get())
                .rem_euclid(std::f64::consts::TAU);
            parameter <= sweep + EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let x = du * cosine + dv * sine;
            let y = -du * sine + dv * cosine;
            let equation = (x / major_radius.get()).powi(2) + (y / minor_radius.get()).powi(2);
            if (equation - 1.0).abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9 {
                return false;
            }
            match bounds {
                Some([start, end]) => {
                    let parameter = ((y / minor_radius.get()).atan2(x / major_radius.get())
                        - start.get())
                    .rem_euclid(std::f64::consts::TAU);
                    let raw = end.get() - start.get();
                    let mut sweep = raw.rem_euclid(std::f64::consts::TAU);
                    if sweep <= EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                        && raw.abs() > EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E12
                    {
                        sweep = std::f64::consts::TAU;
                    }
                    parameter <= sweep + EPS_TYPED_RELATIONS_SKETCH_ENTITY_CONTAINS_POINT_E9
                }
                None => true,
            }
        }
        SketchGeometryDefinition::Hyperbola {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            let cosine = major_angle.get().cos();
            let sine = major_angle.get().sin();
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let x = du * cosine + dv * sine;
            let y = -du * sine + dv * cosine;
            let parameter = (y / minor_radius.get()).asinh();
            let on_curve = (x - major_radius.get() * parameter.cosh()).abs()
                <= SKETCH_POINT_TOLERANCE * (1.0 + x.abs());
            on_curve
                && bounds.as_ref().is_none_or(|[start, end]| {
                    ((*start).min(*end) - SKETCH_POINT_TOLERANCE
                        ..=(*start).max(*end) + SKETCH_POINT_TOLERANCE)
                        .contains(&parameter)
                })
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            let cosine = axis_angle.get().cos();
            let sine = axis_angle.get().sin();
            let du = point.u - vertex.u;
            let dv = point.v - vertex.v;
            let x = du * cosine + dv * sine;
            let parameter = -du * sine + dv * cosine;
            let on_curve = (x - parameter * parameter / (4.0 * focal_length.get())).abs()
                <= SKETCH_POINT_TOLERANCE * (1.0 + x.abs());
            on_curve
                && bounds.as_ref().is_none_or(|[start, end]| {
                    ((*start).min(*end) - SKETCH_POINT_TOLERANCE
                        ..=(*start).max(*end) + SKETCH_POINT_TOLERANCE)
                        .contains(&parameter)
                })
        }
        SketchGeometryDefinition::Point { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::Nurbs { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => false,
    }
}
fn same_dimension_length(left: f64, right: f64) -> bool {
    (left - right).abs()
        <= EPS_RELATION_LOCI_SAME_DIMENSION_LENGTH_E9 * left.abs().max(right.abs()).max(1.0)
}

#[test]fn off_ellipse_accepted(){let geometry=SketchGeometryDefinition::Ellipse{center:Point2::new(-1e308,0.),major_angle:Angle::new(0.).unwrap(),major_radius:Length::new(1.).unwrap(),minor_radius:Length::new(0.5).unwrap(),bounds:None}.try_into().unwrap();let got=sketch_entity_contains_point(&SketchEntity{geometry},Point2::new(1e308,0.));println!("SLD ellipse centered at -1e308 contains +1e308={got}; radii=(1,0.5)");assert!(got);}}
mod catia_chart {use super::*;use cadmpeg_ir::geometry::{SurfaceGeometry,SolvedSurfaceGeometry};const CONSOLIDATED_SITE_TOLERANCE:f64=2e-3;enum ConsolidatedCarrierChart<'a>{Rigid{linear:[[f64;2];2],offset:[f64;2]},Unused(&'a ())}impl ConsolidatedCarrierChart<'_>{fn point(&self,[u,v]:[f64;2])->[f64;2]{match self{Self::Rigid{linear,offset}=>[linear[0][0]*u+linear[0][1]*v+offset[0],linear[1][0]*u+linear[1][1]*v+offset[1]],Self::Unused(_)=>unreachable!()}}}
fn solve_planar_chart_rechart(
    sites: &[[f64; 2]],
    loci: &[Point3],
    target: &SurfaceGeometry,
) -> Option<ConsolidatedCarrierChart<'static>> {
    if !matches!(
        target,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ) || sites.len() != loci.len()
    {
        return None;
    }
    // Target-chart image of each locus. A locus off the plane has no image,
    // because the plane inverse discards the normal component.
    let images = loci
        .iter()
        .map(|locus| {
            let uv = cadmpeg_ir::eval::analytic_surface_parameters(target, *locus)?;
            let back = cadmpeg_ir::eval::surface_point(target, uv.u, uv.v)?;
            ((back.x - locus.x)
                .hypot(back.y - locus.y)
                .hypot(back.z - locus.z)
                <= CONSOLIDATED_SITE_TOLERANCE)
                .then_some([uv.u, uv.v])
        })
        .collect::<Option<Vec<_>>>()?;
    let count = images.len();
    if count < 2 {
        return None;
    }
    let scale = 1.0 / count as f64;
    let mean = |values: &[[f64; 2]]| {
        values.iter().fold([0.0, 0.0], |acc, value| {
            [acc[0] + value[0] * scale, acc[1] + value[1] * scale]
        })
    };
    let stored_center = mean(sites);
    let image_center = mean(&images);
    // Two-dimensional orthogonal Procrustes. `dot` and `cross` accumulate the
    // rotation's cosine and sine lanes; the reflected solution swaps the sign
    // of the image's second chart coordinate.
    let (mut dot, mut cross) = (0.0, 0.0);
    let (mut reflected_dot, mut reflected_cross) = (0.0, 0.0);
    for (stored, image) in sites.iter().zip(&images) {
        let [su, sv] = [stored[0] - stored_center[0], stored[1] - stored_center[1]];
        let [iu, iv] = [image[0] - image_center[0], image[1] - image_center[1]];
        dot += su * iu + sv * iv;
        cross += su * iv - sv * iu;
        reflected_dot += su * iu - sv * iv;
        reflected_cross += su * iv + sv * iu;
    }
    let candidates = [(dot, cross, 1.0), (reflected_dot, reflected_cross, -1.0)];
    let mut admissible = Vec::new();
    for (dot, cross, determinant) in candidates {
        let norm = dot.hypot(cross);
        if !norm.is_finite() || norm <= f64::EPSILON {
            continue;
        }
        let (cosine, sine) = (dot / norm, cross / norm);
        // A reflection about the target chart's first axis follows the
        // rotation, so its second column changes sign.
        let linear = [[cosine, -sine * determinant], [sine, cosine * determinant]];
        let offset = [
            image_center[0] - (linear[0][0] * stored_center[0] + linear[0][1] * stored_center[1]),
            image_center[1] - (linear[1][0] * stored_center[0] + linear[1][1] * stored_center[1]),
        ];
        let chart = ConsolidatedCarrierChart::Rigid { linear, offset };
        let residual = sites
            .iter()
            .zip(&images)
            .map(|(stored, image)| {
                let mapped = chart.point(*stored);
                (mapped[0] - image[0]).hypot(mapped[1] - image[1])
            })
            .fold(0.0f64, f64::max);
        if !residual.is_finite() {
            continue;
        }
        if residual <= CONSOLIDATED_SITE_TOLERANCE {
            admissible.push(chart);
        }
    }
    if admissible.len() == 1 {
        admissible.pop()
    } else {
        None
    }
}

#[test]fn identity_rechart(){let target=SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.)).unwrap()));for a in [1.,1e200]{let sites=[[a,0.],[0.,a],[-a,0.],[0.,-a]];let loci=sites.map(|p|Point3::new(p[0],p[1],0.));let got=solve_planar_chart_rechart(&sites,&loci,&target);println!("CATIA identity chart scale={a:e}: accepted={}",got.is_some());assert_eq!(got.is_some(),a==1.);}}}
