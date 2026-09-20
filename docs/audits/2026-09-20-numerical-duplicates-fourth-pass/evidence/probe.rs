#![allow(dead_code,unused_imports)]
use cadmpeg_ir::math::{Point2,Point3,Vector3};
use cadmpeg_ir::geometry::{SurfaceGeometry,SolvedSurfaceGeometry,nurbs::NurbsSurface};
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::sketches::{SketchGeometry,SketchGeometryDefinition,SpatialSketchGeometry,SpatialSketchGeometryDefinition};
use cadmpeg_ir::scalar::{Angle,Length};
fn line(a:[f64;2],b:[f64;2])->SketchGeometry {SketchGeometryDefinition::Line {start:Point2::new(a[0],a[1]),end:Point2::new(b[0],b[1])}.try_into().unwrap()}
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
mod ir {use super::*;use crate::math::sum::ExactSignedSum;#[derive(Debug,Clone,Copy)]struct ScalarSweepDifferential{value:f64,derivative:f64}
fn scalar_unary_sweep_law_differential(
    operator: &str,
    operand: ScalarSweepDifferential,
) -> Option<ScalarSweepDifferential> {
    let x = operand.value;
    if !x.is_finite() || !operand.derivative.is_finite() {
        return None;
    }
    match operator {
        "ARCTAN" | "ARCOT" | "ARCSEC" | "ARCCSC" | "ARCCSCH" => {
            let mut denominator = ExactSignedSum::default();
            let (value, sign) = match operator {
                "ARCTAN" | "ARCOT" => {
                    denominator.add_product(x, x);
                    denominator.add_product(1.0, 1.0);
                    if operator == "ARCTAN" {
                        (x.atan(), 1.0)
                    } else {
                        (std::f64::consts::FRAC_PI_2 - x.atan(), -1.0)
                    }
                }
                "ARCSEC" | "ARCCSC" => {
                    if x.abs() <= 1.0 {
                        return None;
                    }
                    let factor = (((x.abs() - 1.0) / x.abs()) * (1.0 + 1.0 / x.abs())).sqrt();
                    denominator.add_factors([x.abs(), x.abs(), factor]);
                    if operator == "ARCSEC" {
                        ((1.0 / x).acos(), 1.0)
                    } else {
                        ((1.0 / x).asin(), -1.0)
                    }
                }
                _ => {
                    if x == 0.0 {
                        return None;
                    }
                    denominator.add_product(x.abs(), x.hypot(1.0));
                    let inverse = 1.0 / x;
                    let value = if inverse.is_finite() {
                        inverse.asinh()
                    } else {
                        (std::f64::consts::LN_2 - x.abs().ln()).copysign(x)
                    };
                    (value, -1.0)
                }
            };
            let derivative = match crate::math::sum::scaled_finite(operand.derivative) {
                Some(numerator) => sign * numerator.quotient(denominator.finish()?)?,
                None if operand.derivative == 0.0 => 0.0,
                None => return None,
            };
            return finite_sweep_differential(value, derivative);
        }
        "COTH" | "SECH" | "CSCH" => {
            if x == 0.0 && operator != "SECH" {
                return None;
            }
            let tail = (-x.abs()).exp();
            let sinh_denominator = -(-2.0 * x.abs()).exp_m1();
            let mut numerator = ExactSignedSum::default();
            let mut denominator = ExactSignedSum::default();
            let value = match operator {
                "COTH" => {
                    numerator.add_factors([-4.0, tail, tail, operand.derivative]);
                    denominator.add_product(sinh_denominator, sinh_denominator);
                    1.0 / x.tanh()
                }
                "SECH" => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    numerator.add_factors([
                        -2.0 * x.tanh(),
                        half_tail,
                        half_tail,
                        operand.derivative,
                    ]);
                    denominator.add_product(1.0 + tail * tail, 1.0);
                    2.0 * tail / (1.0 + tail * tail)
                }
                _ => {
                    let half_tail = (-0.5 * x.abs()).exp();
                    numerator.add_factors([
                        -2.0 * (1.0 + tail * tail),
                        half_tail,
                        half_tail,
                        operand.derivative,
                    ]);
                    denominator.add_product(sinh_denominator, sinh_denominator);
                    (2.0 * tail / sinh_denominator).copysign(x)
                }
            };
            let derivative = match numerator.finish() {
                Some(value) => value.quotient(denominator.finish()?)?,
                None => 0.0,
            };
            return finite_sweep_differential(value, derivative);
        }
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
        "ARCCOS" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(-1.0 / denominator)?
        }
        "ARCSIN" => {
            let denominator = (1.0 - x * x).sqrt();
            (denominator > 0.0).then_some(1.0 / denominator)?
        }
        "ARCTANH" => (x.abs() < 1.0).then_some(1.0 / (1.0 - x * x))?,
        "ARCSECH" => {
            let denominator = (1.0 - x * x).sqrt();
            (x > 0.0 && x < 1.0 && denominator > 0.0).then_some(-1.0 / (x * denominator))?
        }
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
            "ARCCOS" => x.acos(),
            "ARCSIN" => x.asin(),
            "ARCTANH" => x.atanh(),
            "ARCSECH" => (1.0 / x).acosh(),
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

#[test]fn remaining_chain_rules(){for (op,x,dx) in [("LN",1e-310,1e-310),("COT",1e-200,1e-200),("CSC",1e-200,1e-200),("ARCSECH",1e-310,1e-310),("EXP",-750.,1e300)]{let got=scalar_unary_sweep_law_differential(op,ScalarSweepDifferential{value:x,derivative:dx});println!("{op} x={x:e} dx={dx:e}: {got:?}");assert!(got.is_none_or(|d|d.derivative==0.));}}
}
mod catia {use super::*;fn point_on_nurbs_surface(_:Point3,_:&NurbsSurface)->Option<bool>{unreachable!()}fn point_on_surface_if_supported(point: Point3, surface: &SurfaceGeometry) -> Option<bool> {
    const TOLERANCE: f64 = 1e-3;
    let residual = match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            point.vector_from(*origin).dot(*normal).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.axis();
            let radius = cylinder_surface.radius();
            let axial = point.vector_from(*origin).dot(*axis);
            let radial = point.distance_squared(*origin) - axial * axial;
            (radial.max(0.0).sqrt() - radius).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin();
            let axis = cone_surface.axis();
            let radius = cone_surface.radius();
            let half_angle = cone_surface.half_angle();
            let axial = point.vector_from(*origin).dot(*axis);
            let radial = (point.distance_squared(*origin) - axial * axial)
                .max(0.0)
                .sqrt();
            (radial - (radius + axial * half_angle.tan()).abs()).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center();
            let radius = sphere_surface.radius();
            (point.distance_squared(*center).sqrt() - radius.abs()).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center();
            let axis = torus_surface.axis();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
            let axial = point.vector_from(*center).dot(*axis);
            let radial = (point.distance_squared(*center) - axial * axial)
                .max(0.0)
                .sqrt();
            (((radial - major_radius).powi(2) + axial * axial).sqrt() - minor_radius.abs()).abs()
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
            return point_on_nurbs_surface(point, surface);
        }
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed { .. }
            | SolvedSurfaceGeometry::Unknown { .. },
        )
        | SurfaceGeometry::Procedural { .. } => return None,
    };
    Some(residual <= TOLERANCE)
}

#[test]fn axial_cancellation(){let sf=SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(CylinderSurface::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.),1.).unwrap()));for z in [0.,1e8]{let got=point_on_surface_if_supported(Point3::new(1.,0.,z),&sf);println!("CATIA radius-one cylinder, exact point (1,0,{z}): {got:?}");assert_eq!(got,Some(z==0.));}}
}
mod f3d {use super::*;const EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9:f64=1e-9;const EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9:f64=1e-9;const EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9:f64=1e-9;fn spatial_point_distance_matches(
    first: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    second: &cadmpeg_ir::sketches::SpatialSketchGeometry,
    expected: f64,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let (
        SpatialSketchGeometryDefinition::Point { position: first },
        SpatialSketchGeometryDefinition::Point { position: second },
    ) = (first.definition(), second.definition())
    else {
        return false;
    };
    let measured = ((second.x - first.x).powi(2)
        + (second.y - first.y).powi(2)
        + (second.z - first.z).powi(2))
    .sqrt();
    let scale = 1.0 + measured.max(expected.abs());
    expected.is_finite()
        && (measured - expected.abs()).abs()
            <= EPS_DIMENSIONS_SPATIAL_POINT_DISTANCE_MATCHES_E9 * scale
}
fn point_lies_on_sketch_geometry(
    point: Point2,
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let close = |left: f64, right: f64| {
        (left - right).abs()
            <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 * (1.0 + left.abs().max(right.abs()))
    };
    match geometry.definition() {
        SketchGeometryDefinition::Point { position } => sketch_points_close(point, *position),
        SketchGeometryDefinition::Line { start, end } => {
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let length_squared = direction.u.mul_add(direction.u, direction.v * direction.v);
            if length_squared <= 1.0e-18 {
                return false;
            }
            let relative = Point2::new(point.u - start.u, point.v - start.v);
            let parameter =
                relative.u.mul_add(direction.u, relative.v * direction.v) / length_squared;
            let cross = relative.u.mul_add(direction.v, -relative.v * direction.u);
            (-EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                ..=1.0 + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9)
                .contains(&parameter)
                && cross.abs()
                    <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                        * (1.0 + length_squared.sqrt())
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let length = direction.u.hypot(direction.v);
            if length <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 {
                return false;
            }
            let relative = Point2::new(point.u - origin.u, point.v - origin.v);
            relative
                .u
                .mul_add(direction.v, -relative.v * direction.u)
                .abs()
                <= EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9 * (1.0 + length)
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            close((point.u - center.u).hypot(point.v - center.v), radius.get())
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            close(relative.u.hypot(relative.v), radius.get())
                && angle_in_sweep(
                    relative.v.atan2(relative.u),
                    start_angle.get(),
                    end_angle.get(),
                    EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9,
                )
        }
        SketchGeometryDefinition::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            bounds,
        } => {
            if major_radius.get() <= 0.0 || minor_radius.get() <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.get();
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.get();
            close(x.mul_add(x, y * y), 1.0)
                && match bounds {
                    Some([start, end]) => angle_in_sweep(
                        y.atan2(x),
                        start.get(),
                        end.get(),
                        EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9,
                    ),
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
            if major_radius.get() <= 0.0 || minor_radius.get() <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - center.u, point.v - center.v);
            let (sin, cos) = major_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin) / major_radius.get();
            let y = (-relative.u).mul_add(sin, relative.v * cos) / minor_radius.get();
            let parameter = y.asinh();
            close(x, parameter.cosh())
                && match bounds {
                    Some([start, end]) => {
                        parameter >= *start - EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                            && parameter <= *end + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    }
                    None => true,
                }
        }
        SketchGeometryDefinition::Parabola {
            vertex,
            axis_angle,
            focal_length,
            bounds,
        } => {
            if focal_length.get() <= 0.0 {
                return false;
            }
            let relative = Point2::new(point.u - vertex.u, point.v - vertex.v);
            let (sin, cos) = axis_angle.get().sin_cos();
            let x = relative.u.mul_add(cos, relative.v * sin);
            let y = (-relative.u).mul_add(sin, relative.v * cos);
            let parameter = y / (2.0 * focal_length.get());
            close(x, focal_length.get() * parameter * parameter)
                && match bounds {
                    Some([start, end]) => {
                        parameter >= *start - EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                            && parameter <= *end + EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                    }
                    None => true,
                }
        }
        SketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let tolerance = EPS_DIMENSIONS_POINT_LIES_ON_SKETCH_GEOMETRY_E9
                * (1.0 + point.u.abs().max(point.v.abs()));
            let control_points = curve.control_points();
            let weights = curve.weights();
            cadmpeg_ir::eval::nurbs_pcurve_contains_point(
                curve.degree(),
                curve.knots(),
                &control_points,
                weights.as_deref(),
                point,
                tolerance,
            )
            .unwrap_or(false)
        }
        SketchGeometryDefinition::Nurbs { .. }
        | SketchGeometryDefinition::Text { .. }
        | SketchGeometryDefinition::ExternalReference { .. }
        | SketchGeometryDefinition::Native { .. } => false,
    }
}
fn sketch_points_close(first: Point2, second: Point2) -> bool {
    let scale = 1.0
        + first
            .u
            .abs()
            .max(first.v.abs())
            .max(second.u.abs())
            .max(second.v.abs());
    (first.u - second.u).abs() <= scale * EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9
        && (first.v - second.v).abs() <= scale * EPS_DIMENSIONS_SKETCH_POINTS_CLOSE_E9
}
fn angle_in_sweep(angle: f64, start: f64, end: f64, tolerance: f64) -> bool {
    let sweep = end - start;
    if sweep.abs() >= std::f64::consts::TAU - tolerance {
        return true;
    }
    if sweep >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep + tolerance
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU) <= -sweep + tolerance
    }
}

#[test]fn incorrect_spatial_dimension(){let p=|x|SpatialSketchGeometryDefinition::Point{position:Point3::new(x,0.,0.)}.try_into().unwrap();let got=spatial_point_distance_matches(&p(0.),&p(1e200),1.);println!("F3D 1e200 separation accepted for dimension 1: {got}");assert!(got);}
#[test]fn short_line_membership(){let got=point_lies_on_sketch_geometry(Point2::new(0.5e-6,1e-4),&line([0.,0.],[1e-6,0.]));println!("F3D point 1e-4 off 1e-6 segment, tolerance 1e-9: accepted={got}");assert!(got);}
#[test]fn remote_ellipse_membership(){let e=SketchGeometryDefinition::Ellipse{center:Point2::new(0.,0.),major_angle:Angle::new(0.).unwrap(),major_radius:Length::new(1.).unwrap(),minor_radius:Length::new(0.5).unwrap(),bounds:None}.try_into().unwrap();let got=point_lies_on_sketch_geometry(Point2::new(1e200,0.),&e);println!("F3D unit ellipse accepts (1e200,0): {got}");assert!(got);}
}
mod creo {use super::*;fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2], tolerance: f64) -> bool {
    let orient = |a: [f64; 2], b: [f64; 2], point: [f64; 2]| {
        (b[0] - a[0]).mul_add(point[1] - a[1], -((b[1] - a[1]) * (point[0] - a[0])))
    };
    let on_segment = |segment: [[f64; 2]; 2], point: [f64; 2]| {
        point[0] >= segment[0][0].min(segment[1][0]) - tolerance
            && point[0] <= segment[0][0].max(segment[1][0]) + tolerance
            && point[1] >= segment[0][1].min(segment[1][1]) - tolerance
            && point[1] <= segment[0][1].max(segment[1][1]) + tolerance
    };
    let orientations = [
        orient(first[0], first[1], second[0]),
        orient(first[0], first[1], second[1]),
        orient(second[0], second[1], first[0]),
        orient(second[0], second[1], first[1]),
    ];
    let first_length = (first[1][0] - first[0][0]).hypot(first[1][1] - first[0][1]);
    let second_length = (second[1][0] - second[0][0]).hypot(second[1][1] - second[0][1]);
    let first_cross_tolerance = tolerance * first_length.max(1.0);
    let second_cross_tolerance = tolerance * second_length.max(1.0);
    let opposite = |left: f64, right: f64, cross_tolerance: f64| {
        (left > cross_tolerance && right < -cross_tolerance)
            || (left < -cross_tolerance && right > cross_tolerance)
    };
    if opposite(orientations[0], orientations[1], first_cross_tolerance)
        && opposite(orientations[2], orientations[3], second_cross_tolerance)
    {
        return true;
    }
    (orientations[0].abs() <= first_cross_tolerance && on_segment(first, second[0]))
        || (orientations[1].abs() <= first_cross_tolerance && on_segment(first, second[1]))
        || (orientations[2].abs() <= second_cross_tolerance && on_segment(second, first[0]))
        || (orientations[3].abs() <= second_cross_tolerance && on_segment(second, first[1]))
}

#[test]fn small_crossing_segments(){for a in [1.,1e-5]{let got=segments_intersect([[-a,0.],[a,0.]],[[0.,-a],[0.,a]],1e-9);println!("Creo perpendicular segment half-length={a:e}: intersect={got}");assert_eq!(got,a==1.);}}
}
mod nx {use super::*;const EPS_OFFSET_SOLVE_DAMPED_LEAST_SQUARES_4X4_E12:f64=1e-12;fn solve_4x4(mut matrix: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    for pivot in 0..4 {
        let row = (pivot..4).max_by(|first, second| {
            matrix[*first][pivot]
                .abs()
                .total_cmp(&matrix[*second][pivot].abs())
        })?;
        if !matrix[row][pivot].is_finite() || matrix[row][pivot].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(pivot, row);
        rhs.swap(pivot, row);
        let pivot_row = matrix[pivot];
        for row in pivot + 1..4 {
            let factor = matrix[row][pivot] / matrix[pivot][pivot];
            for (value, pivot_value) in matrix[row][pivot..].iter_mut().zip(&pivot_row[pivot..]) {
                *value -= factor * pivot_value;
            }
            rhs[row] -= factor * rhs[pivot];
        }
    }
    let mut solution = [0.0; 4];
    for row in (0..4).rev() {
        let known = (row + 1..4)
            .map(|column| matrix[row][column] * solution[column])
            .sum::<f64>();
        solution[row] = (rhs[row] - known) / matrix[row][row];
    }
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}
fn solve_damped_least_squares_4x4(
    matrix: [[f64; 4]; 4],
    rhs: [f64; 4],
) -> Option<[f64; 4]> {
    if !matrix.iter().flatten().all(|value| value.is_finite())
        || !rhs.iter().all(|value| value.is_finite())
    {
        return None;
    }
    let normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| matrix[index][row] * matrix[index][column])
                .sum::<f64>()
        })
    });
    let normal_rhs: [f64; 4] = std::array::from_fn(|column| {
        (0..4)
            .map(|index| matrix[index][column] * rhs[index])
            .sum::<f64>()
    });
    let max_column_scale = (0..4)
        .map(|index| normal[index][index].abs())
        .fold(0.0_f64, f64::max)
        .sqrt();
    if !max_column_scale.is_finite() || max_column_scale == 0.0 {
        return None;
    }
    let column_scales: [f64; 4] = std::array::from_fn(|index| {
        normal[index][index]
            .max(0.0)
            .sqrt()
            .max(max_column_scale * EPS_OFFSET_SOLVE_DAMPED_LEAST_SQUARES_4X4_E12)
    });
    let scaled_normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            normal[row][column] / (column_scales[row] * column_scales[column])
        })
    });
    let scaled_rhs: [f64; 4] =
        std::array::from_fn(|column| normal_rhs[column] / column_scales[column]);
    let initial_error = rhs.iter().map(|value| value * value).sum::<f64>();
    for exponent in -12..=-3 {
        let damping = 10_f64.powi(exponent);
        let mut regularized = scaled_normal;
        for (index, row) in regularized.iter_mut().enumerate() {
            row[index] += damping;
        }
        let Some(scaled_step) = solve_4x4(regularized, scaled_rhs) else {
            continue;
        };
        let step: [f64; 4] = std::array::from_fn(|index| scaled_step[index] / column_scales[index]);
        let linear_error = (0..4)
            .map(|row| {
                let residual = (0..4)
                    .map(|column| matrix[row][column] * step[column])
                    .sum::<f64>()
                    - rhs[row];
                residual * residual
            })
            .sum::<f64>();
        if linear_error.is_finite() && linear_error < initial_error {
            return Some(step);
        }
    }
    None
}

#[test]fn scaled_rank_deficient_solve(){for a in [1.,1e-200,1e200]{let got=solve_damped_least_squares_4x4([[a,0.,0.,0.],[0.,a,0.,0.],[0.,0.,a,0.],[0.,0.,0.,0.]],[a,a,a,0.]);println!("NX rank-three consistent system scale={a:e}: {got:?}; exact step=(1,1,1,0)");assert_eq!(got.is_some(),a==1.);}}
}
mod sld {use super::*;const HELIX_MAX_RELATIVE_RESIDUAL:f64=5e-4;fn fit_helix_polyline(
    points: &[Point3],
    revolutions: f64,
    clockwise: bool,
) -> Option<(Point3, Vector3, f64, f64)> {
    if points.len() < 6 || !revolutions.is_finite() || revolutions <= 0.0 {
        return None;
    }
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(0.0);
    for pair in points.windows(2) {
        let delta = Vector3::new(
            pair[1].x - pair[0].x,
            pair[1].y - pair[0].y,
            pair[1].z - pair[0].z,
        );
        parameters.push(parameters.last().copied()? + delta.norm());
    }
    let total = *parameters.last()?;
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let angle = std::f64::consts::TAU * revolutions * if clockwise { -1.0 } else { 1.0 };
    let mut normal = [[0.0; 4]; 4];
    let mut rhs = [[0.0; 3]; 4];
    for (point, distance) in points.iter().zip(parameters) {
        let t = distance / total;
        let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
        for i in 0..4 {
            for j in 0..4 {
                normal[i][j] += row[i] * row[j];
            }
            rhs[i][0] += row[i] * point.x;
            rhs[i][1] += row[i] * point.y;
            rhs[i][2] += row[i] * point.z;
        }
    }
    let x = solve_four(normal, rhs)?;
    let cosine = Vector3::new(x[2][0], x[2][1], x[2][2]);
    let sine = Vector3::new(x[3][0], x[3][1], x[3][2]);
    let mut axis = cosine.cross(sine);
    let axis_length = axis.norm();
    if !axis_length.is_finite() || axis_length <= 0.0 {
        return None;
    }
    axis = Vector3::new(
        axis.x / axis_length,
        axis.y / axis_length,
        axis.z / axis_length,
    );
    let radial_cosine = subtract_axis(cosine, axis);
    let radial_sine = subtract_axis(sine, axis);
    let radius_estimate = (radial_cosine.norm() + radial_sine.norm()) * 0.5;
    if !radius_estimate.is_finite() || radius_estimate <= 0.0 {
        return None;
    }
    let mut max_error = 0.0f64;
    for (point, distance) in
        points.iter().zip(
            std::iter::once(0.0).chain(points.windows(2).scan(0.0, |sum, pair| {
                let delta = Vector3::new(
                    pair[1].x - pair[0].x,
                    pair[1].y - pair[0].y,
                    pair[1].z - pair[0].z,
                );
                *sum += delta.norm();
                Some(*sum)
            })),
        )
    {
        let t = distance / total;
        let row = [1.0, t, (angle * t).cos(), (angle * t).sin()];
        for (coordinate, actual) in [point.x, point.y, point.z].into_iter().enumerate() {
            let fitted = (0..4).map(|i| row[i] * x[i][coordinate]).sum::<f64>();
            max_error = max_error.max((fitted - actual).abs());
        }
    }
    if max_error > radius_estimate * HELIX_MAX_RELATIVE_RESIDUAL {
        return None;
    }
    let (origin, radius) = fit_circle_on_axis(points, axis)?;
    let displacement = Vector3::new(
        points.last()?.x - points[0].x,
        points.last()?.y - points[0].y,
        points.last()?.z - points[0].z,
    );
    Some((origin, axis, radius, displacement.dot(axis)))
}
fn fit_circle_on_axis(points: &[Point3], axis: Vector3) -> Option<(Point3, f64)> {
    let helper = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let mut u = axis.cross(helper);
    let u_length = u.norm();
    u = Vector3::new(u.x / u_length, u.y / u_length, u.z / u_length);
    let v = axis.cross(u);
    let reference = points[0];
    let mut normal = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for point in points {
        let delta = Vector3::new(
            point.x - reference.x,
            point.y - reference.y,
            point.z - reference.z,
        );
        let x = delta.dot(u);
        let y = delta.dot(v);
        let row = [x, y, 1.0];
        let target = -(x * x + y * y);
        for i in 0..3 {
            rhs[i] += row[i] * target;
            for j in 0..3 {
                normal[i][j] += row[i] * row[j];
            }
        }
    }
    let solution = solve_three(normal, rhs)?;
    let center_u = -solution[0] * 0.5;
    let center_v = -solution[1] * 0.5;
    let radius_squared = center_u * center_u + center_v * center_v - solution[2];
    if !radius_squared.is_finite() || radius_squared <= 0.0 {
        return None;
    }
    Some((
        Point3::new(
            reference.x + center_u * u.x + center_v * v.x,
            reference.y + center_u * u.y + center_v * v.y,
            reference.z + center_u * u.z + center_v * v.z,
        ),
        radius_squared.sqrt(),
    ))
}
fn solve_three(mut matrix: [[f64; 3]; 3], mut rhs: [f64; 3]) -> Option<[f64; 3]> {
    for column in 0..3 {
        let pivot = (column..3).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        rhs[column] /= scale;
        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            let pivot_row = matrix[column];
            for (target, pivot) in matrix[row].iter_mut().zip(pivot_row).skip(column) {
                *target -= factor * pivot;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Some(rhs)
}
fn subtract_axis(vector: Vector3, axis: Vector3) -> Vector3 {
    let axial = vector.dot(axis);
    Vector3::new(
        vector.x - axial * axis.x,
        vector.y - axial * axis.y,
        vector.z - axial * axis.z,
    )
}
fn solve_four(mut matrix: [[f64; 4]; 4], mut rhs: [[f64; 3]; 4]) -> Option<[[f64; 3]; 4]> {
    for column in 0..4 {
        let pivot = (column..4).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= 1.0e-14 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        for value in &mut rhs[column] {
            *value /= scale;
        }
        for row in 0..4 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            let pivot_row = matrix[column];
            for (target, pivot) in matrix[row].iter_mut().zip(pivot_row).skip(column) {
                *target -= factor * pivot;
            }
            let rhs_pivot = rhs[column];
            for (target, pivot) in rhs[row].iter_mut().zip(rhs_pivot) {
                *target -= factor * pivot;
            }
        }
    }
    Some(rhs)
}

#[test]fn small_helix(){for r in [1.,1e-8]{let pts=(0..=16).map(|i|{let t=i as f64/16.;let angle=t*std::f64::consts::TAU;Point3::new(r*angle.cos(),r*angle.sin(),r*t)}).collect::<Vec<_>>();let got=fit_helix_polyline(&pts,1.,false);println!("SLD exact helix radius={r:e}: {got:?}");assert_eq!(got.is_some(),r==1.);}}
}
mod expressions {use super::*;const EPS_SKETCHES_CHECK_SKETCHES_E9:f64=1e-9;
#[test]fn ir_distance_admission(){let first=Point3::new(0.,0.,0.);let second=Point3::new(1e200,0.,0.);let expected=1.;let measured=((second.x - first.x).powi(2)
                            + (second.y - first.y).powi(2)
                            + (second.z - first.z).powi(2))
                        .sqrt();let scale=1.0+measured.max(expected);let got=(measured - expected).abs() <= EPS_SKETCHES_CHECK_SKETCHES_E9 * scale;println!("IR spatial separation=1e200 expected=1 measured={measured} accepted={got}");assert!(got);}
#[test]fn radius_keys(){let key=|radius:f64|{let quantum=1e-6;(radius / quantum).round() as i64};let a=key(1e14);let b=key(2e14);println!("SLD radius keys for 1e14 and 2e14: {a}, {b}");assert_eq!(a,b);}
#[test]fn freecad_default_extrusion_length(){for x in [1e200]{let direction=Vector3::new(x,0.,0.);let got=
            (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z)
                .sqrt()
        ;println!("FreeCAD extrusion Dir=({x:e},0,0), magnitude={got:e}");assert_ne!(got,x);assert!(direction.unit().is_some());}}
#[test]fn iges_conic_coefficients(){for major_radius in [1e-200_f64,1e200]{let got=1.0/(major_radius*major_radius);println!("IGES conic radius={major_radius:e} coefficient={got}");assert!(got==0.||!got.is_finite());}}
}
mod ir_more {use super::*;use cadmpeg_ir::transform::Transform;fn affine_point(transform: Transform, point: Point3) -> Point3 {
    let rows = transform.rows();
    Point3::new(
        rows[0][0] * point.x + rows[0][1] * point.y + rows[0][2] * point.z + rows[0][3],
        rows[1][0] * point.x + rows[1][1] * point.y + rows[1][2] * point.z + rows[1][3],
        rows[2][0] * point.x + rows[2][1] * point.y + rows[2][2] * point.z + rows[2][3],
    )
}
fn affine_vector(transform: Transform, vector: Vector3) -> Vector3 {
    let rows = transform.rows();
    Vector3::new(
        rows[0][0] * vector.x + rows[0][1] * vector.y + rows[0][2] * vector.z,
        rows[1][0] * vector.x + rows[1][1] * vector.y + rows[1][2] * vector.z,
        rows[2][0] * vector.x + rows[2][1] * vector.y + rows[2][2] * vector.z,
    )
}
fn affine_orientation(transform: Transform) -> f64 {
    let [first, second, third, _] = transform.rows();
    let determinant = first[0] * (second[1] * third[2] - second[2] * third[1])
        - first[1] * (second[0] * third[2] - second[2] * third[0])
        + first[2] * (second[0] * third[1] - second[1] * third[0]);
    if determinant.is_finite() && determinant < 0.0 {
        -1.0
    } else {
        1.0
    }
}
fn polyline_point(points: &[Point3], parameters: &[f64], t: f64) -> Option<Point3> {
    if points.len() < 2 || !t.is_finite() {
        return None;
    }
    let segment = parameters.windows(2).position(|window| {
        (t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])
    })?;
    let width = parameters[segment + 1] - parameters[segment];
    if width == 0.0 || !width.is_finite() {
        return None;
    }
    let fraction = (t - parameters[segment]) / width;
    let start = points[segment];
    let end = points[segment + 1];
    Some(Point3::new(
        start.x + fraction * (end.x - start.x),
        start.y + fraction * (end.y - start.y),
        start.z + fraction * (end.z - start.z),
    ))
}
fn polyline_tangent(points: &[Point3], parameters: &[f64], t: f64) -> Option<Vector3> {
    if points.len() < 2 || !t.is_finite() {
        return None;
    }
    let mut tangent = None;
    for (segment, window) in parameters.windows(2).enumerate() {
        if !((t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])) {
            continue;
        }
        let width = window[1] - window[0];
        if width == 0.0 || !width.is_finite() {
            return None;
        }
        let start = points[segment];
        let end = points[segment + 1];
        let candidate = Vector3::new(
            (end.x - start.x) / width,
            (end.y - start.y) / width,
            (end.z - start.z) / width,
        );
        if tangent.is_some_and(|tangent| tangent != candidate) {
            return None;
        }
        tangent = Some(candidate);
    }
    tangent
}

#[test]fn affine_cancel(){let tr=Transform::affine([[1e308,-1e308,1.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.]]).unwrap();let p=Point3::new(2.,2.,3.);let got=affine_point(tr,p);let robust=tr.apply_point(p).unwrap();println!("IR evaluator affine point={got:?}; shared Transform={robust:?}");assert!(!got.is_finite());assert_eq!(robust.x,3.);let v=affine_vector(tr,Vector3::new(2.,2.,3.));assert!(!v.is_finite());}
#[test]fn reflection_orientation(){for a in [1.,1e-200,1e200]{let tr=Transform::affine([[-a,0.,0.,0.],[0.,a,0.,0.],[0.,0.,a,0.]]).unwrap();let got=affine_orientation(tr);println!("IR scaled reflection scale={a:e}: orientation={got}, expected=-1");assert_eq!(got,if a==1.{-1.}else{1.});}}
#[test]fn polyline_interpolation(){let p=[Point3::new(-1e308,0.,0.),Point3::new(1e308,0.,0.)];let got=polyline_point(&p,&[0.,1.],0.5);println!("IR symmetric polyline midpoint={got:?}, expected origin");assert!(got.is_some_and(|p|!p.is_finite()));let q=polyline_point(&[Point3::new(0.,0.,0.),Point3::new(1.,0.,0.)],&[-1e308,1e308],0.);println!("IR finite wide-domain midpoint={q:?}, expected (0.5,0,0)");assert!(q.is_none());}
#[test]fn polar_scale(){use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,PolarHarmonicPcurve};for a in [1.,1e-200,1e200]{let g=PcurveGeometry::PolarHarmonic(PolarHarmonicPcurve::try_new(Point2::new(0.,0.),Point2::new(a,0.),Point2::new(0.,a),0.,0.,0.).unwrap());let p=cadmpeg_ir::eval::pcurve_uv(&g,0.);let d=cadmpeg_ir::eval::pcurve_tangent(&g,0.);println!("IR polar harmonic scale={a:e}: point={p:?}, tangent={d:?}; expected (0,0), (1,0)");assert_eq!(d.is_some(),a==1.);}}
#[test]fn great_circle_derivative(){use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,SphericalGreatCirclePcurve};let g=PcurveGeometry::SphericalGreatCircle(SphericalGreatCirclePcurve::try_new(0.,1.,0.,1e200).unwrap());let d=cadmpeg_ir::eval::pcurve_tangent(&g,0.5).unwrap();println!("IR great-circle slope=1e200 at 0.5: tangent={d:?}; latitude derivative ~-6.225e-201");assert_eq!(d.v,0.);}
}
mod planar {use super::*;pub fn line_circle_parameters(
    start: Point2,
    end: Point2,
    center: Point2,
    radius: f64,
) -> Option<[f64; 2]> {
    if !start.is_finite()
        || !end.is_finite()
        || !center.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
    {
        return None;
    }
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let relative = Point2::new(start.u - center.u, start.v - center.v);
    let scale = direction
        .u
        .abs()
        .max(direction.v.abs())
        .max(relative.u.abs())
        .max(relative.v.abs())
        .max(radius);
    if !scale.is_finite() {
        return None;
    }
    let d = Point2::new(direction.u / scale, direction.v / scale);
    let o = Point2::new(relative.u / scale, relative.v / scale);
    let r = radius / scale;
    let a = d.u.mul_add(d.u, d.v * d.v);
    if a == 0.0 {
        return None;
    }
    let b = 2.0 * o.u.mul_add(d.u, o.v * d.v);
    let c = o.u.mul_add(o.u, o.v * o.v) - r * r;
    let discriminant = b.mul_add(b, -4.0 * a * c);
    let error = 64.0 * f64::EPSILON * (b * b + (4.0 * a * c).abs());
    if discriminant < -error {
        return None;
    }
    let root = discriminant.max(0.0).sqrt();
    let q = -0.5 * (b + root.copysign(b));
    let parameters = if root == 0.0 {
        [-b / (2.0 * a); 2]
    } else {
        [q / a, c / q]
    };
    parameters
        .iter()
        .all(|p| p.is_finite())
        .then_some(parameters)
}
pub fn circle_intersections(
    first: Point2,
    first_radius: f64,
    second: Point2,
    second_radius: f64,
) -> Option<Vec<Point2>> {
    if !first.is_finite()
        || !second.is_finite()
        || !first_radius.is_finite()
        || !second_radius.is_finite()
        || first_radius <= 0.0
        || second_radius <= 0.0
    {
        return None;
    }
    let delta = Point2::new(second.u - first.u, second.v - first.v);
    let scale = delta
        .u
        .abs()
        .max(delta.v.abs())
        .max(first_radius)
        .max(second_radius);
    if !scale.is_finite() {
        return None;
    }
    let delta = Point2::new(delta.u / scale, delta.v / scale);
    let distance = delta.u.hypot(delta.v);
    if distance == 0.0 {
        return (first_radius != second_radius).then(Vec::new);
    }
    let r = first_radius / scale;
    let s = second_radius / scale;
    if distance > r + s || distance < (r - s).abs() {
        return Some(Vec::new());
    }
    let along = 0.5 * (distance + (r - s) * (r + s) / distance);
    let height_squared = (r - along) * (r + along);
    let error = 64.0 * f64::EPSILON * (r * r + along * along);
    if height_squared < -error {
        return Some(Vec::new());
    }
    let height = height_squared.max(0.0).sqrt();
    let unit = Point2::new(delta.u / distance, delta.v / distance);
    let mut points = Vec::new();
    for height in [height, -height] {
        let point = Point2::new(
            first.u + (along * unit.u - height * unit.v) * scale,
            first.v + (along * unit.v + height * unit.u) * scale,
        );
        if !point.is_finite() {
            return None;
        }
        if !points.contains(&point) {
            points.push(point);
        }
    }
    Some(points)
}

#[test]fn finite_intersections_lost(){let roots=line_circle_parameters(Point2::new(0.,0.),Point2::new(1e-200,0.),Point2::new(0.,0.),1.);println!("IR tiny line at unit-circle center: roots={roots:?}, expected +/-1e200");assert!(roots.is_none());let tangent=circle_intersections(Point2::new(-1e308,0.),1e308,Point2::new(1e308,0.),1e308);println!("IR radius 1e308 externally tangent circles: {tangent:?}, expected origin");assert!(tangent.is_none());}
}
mod iges_eval {use super::*;use cadmpeg_core::decode::alloc_filled;use cadmpeg_ir::geometry::{pcurve::{PcurveGeometry,ParabolaPcurve,PcurveNurbs,PcurveNurbsPoles},CurveGeometry,SolvedCurveGeometry};fn basis(knots: &[f64], degree: usize, count: usize, parameter: f64) -> Option<Vec<f64>> {
    if count == 0 || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let last = count - 1;
    let span = if parameter == knots[count] {
        last
    } else {
        (degree..count).find(|index| knots[*index] <= parameter && parameter < knots[*index + 1])?
    };
    let degree_slots = degree.checked_add(1)?;
    let mut values = alloc_filled(degree_slots, 0.0, "iges basis values").ok()?;
    let mut left = alloc_filled(degree_slots, 0.0, "iges basis left knots").ok()?;
    let mut right = alloc_filled(degree_slots, 0.0, "iges basis right knots").ok()?;
    values[0] = 1.0;
    for order in 1..=degree {
        left[order] = parameter - knots[span + 1 - order];
        right[order] = knots[span + order] - parameter;
        let mut saved = 0.0;
        for index in 0..order {
            let denominator = right[index + 1] + left[order - index];
            let term = if denominator == 0.0 {
                0.0
            } else {
                values[index] / denominator
            };
            values[index] = saved + right[index + 1] * term;
            saved = left[order - index] * term;
        }
        values[order] = saved;
    }
    let mut result = alloc_filled(count, 0.0, "iges basis result").ok()?;
    for (offset, value) in values.into_iter().enumerate() {
        result[span - degree + offset] = value;
    }
    Some(result)
}
fn pcurve(geometry: &PcurveGeometry, parameter: f64) -> Option<Point2> {
    match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin();
            let direction = line_pcurve.direction();
            Some(Point2::new(
                origin.u + parameter * direction.u,
                origin.v + parameter * direction.v,
            ))
        }
        PcurveGeometry::Circle(circle_pcurve) => {
            let center = circle_pcurve.center();
            let x_axis = circle_pcurve.x_axis();
            let y_axis = circle_pcurve.y_axis();
            let radius = circle_pcurve.radius();
            Some(Point2::new(
                center.u + radius * (x_axis.u * parameter.cos() + y_axis.u * parameter.sin()),
                center.v + radius * (x_axis.v * parameter.cos() + y_axis.v * parameter.sin()),
            ))
        }
        PcurveGeometry::Ellipse(ellipse_pcurve) => {
            let center = ellipse_pcurve.center();
            let x_axis = ellipse_pcurve.x_axis();
            let y_axis = ellipse_pcurve.y_axis();
            let major_radius = ellipse_pcurve.major_radius();
            let minor_radius = ellipse_pcurve.minor_radius();
            Some(Point2::new(
                center.u
                    + major_radius * x_axis.u * parameter.cos()
                    + minor_radius * y_axis.u * parameter.sin(),
                center.v
                    + major_radius * x_axis.v * parameter.cos()
                    + minor_radius * y_axis.v * parameter.sin(),
            ))
        }
        PcurveGeometry::Harmonic(harmonic_pcurve) => {
            let center = harmonic_pcurve.center();
            let cosine = harmonic_pcurve.cosine();
            let sine = harmonic_pcurve.sine();
            Some(Point2::new(
                center.u + cosine.u * parameter.cos() + sine.u * parameter.sin(),
                center.v + cosine.v * parameter.cos() + sine.v * parameter.sin(),
            ))
        }
        PcurveGeometry::Parabola(parabola_pcurve) => {
            let vertex = parabola_pcurve.vertex();
            let x_axis = parabola_pcurve.x_axis();
            let y_axis = parabola_pcurve.y_axis();
            let focal_distance = parabola_pcurve.focal_distance();
            Some(Point2::new(
                vertex.u
                    + focal_distance * x_axis.u * parameter * parameter
                    + 2.0 * focal_distance * y_axis.u * parameter,
                vertex.v
                    + focal_distance * x_axis.v * parameter * parameter
                    + 2.0 * focal_distance * y_axis.v * parameter,
            ))
        }
        PcurveGeometry::Hyperbola(hyperbola_pcurve) => {
            let center = hyperbola_pcurve.center();
            let x_axis = hyperbola_pcurve.x_axis();
            let y_axis = hyperbola_pcurve.y_axis();
            let major_radius = hyperbola_pcurve.major_radius();
            let minor_radius = hyperbola_pcurve.minor_radius();
            Some(Point2::new(
                center.u
                    + major_radius * x_axis.u * parameter.cosh()
                    + minor_radius * y_axis.u * parameter.sinh(),
                center.v
                    + major_radius * x_axis.v * parameter.cosh()
                    + minor_radius * y_axis.v * parameter.sinh(),
            ))
        }
        PcurveGeometry::Hyperbolic(hyperbolic_pcurve) => {
            let center = hyperbolic_pcurve.center();
            let cosine = hyperbolic_pcurve.cosine();
            let sine = hyperbolic_pcurve.sine();
            Some(Point2::new(
                center.u + cosine.u * parameter.cosh() + sine.u * parameter.sinh(),
                center.v + cosine.v * parameter.cosh() + sine.v * parameter.sinh(),
            ))
        }
        PcurveGeometry::Nurbs { nurbs } => {
            let values = basis(
                nurbs.knots(),
                usize::try_from(nurbs.degree()).ok()?,
                nurbs.control_points().len(),
                parameter,
            )?;
            let mut u = 0.0;
            let mut v = 0.0;
            let mut denominator = 0.0;
            for (index, value) in values.into_iter().enumerate() {
                let weight = nurbs.weights().map_or(1.0, |weights| weights[index]);
                let coefficient = value * weight;
                u += coefficient * nurbs.control_points()[index].u;
                v += coefficient * nurbs.control_points()[index].v;
                denominator += coefficient;
            }
            (denominator != 0.0).then(|| Point2::new(u / denominator, v / denominator))
        }
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let basis = trimmed_pcurve.basis();
            let parameter = parameter.clamp(
                parameter_range[0].min(parameter_range[1]),
                parameter_range[0].max(parameter_range[1]),
            );
            pcurve(basis, parameter)
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            let distance = offset_pcurve.distance();
            let basis = offset_pcurve.basis();
            let delta = f64::EPSILON.sqrt() * parameter.abs().max(1.0);
            let point = pcurve(basis, parameter)?;
            let before = pcurve(basis, parameter - delta)?;
            let after = pcurve(basis, parameter + delta)?;
            let du = after.u - before.u;
            let dv = after.v - before.v;
            let magnitude = du.hypot(dv);
            (magnitude > 0.0).then(|| {
                Point2::new(
                    point.u - distance * dv / magnitude,
                    point.v + distance * du / magnitude,
                )
            })
        }
        PcurveGeometry::Transformed { basis, transform } => {
            pcurve(basis, parameter).map(|point| transform.apply_point(point))
        }
        PcurveGeometry::PolarHarmonic(_) => cadmpeg_ir::eval::pcurve_uv(geometry, parameter),
        PcurveGeometry::PolarNurbs { .. } => cadmpeg_ir::eval::pcurve_uv(geometry, parameter),
        PcurveGeometry::SphericalGreatCircle(_) => cadmpeg_ir::eval::pcurve_uv(geometry, parameter),
    }
}
fn curve(geometry: &CurveGeometry, parameter: f64) -> Option<Point3> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin();
            let direction = line_curve.direction();
            Some(origin.translated(*direction, parameter))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center();
            let axis = circle_curve.axis();
            let ref_direction = circle_curve.ref_direction();
            let radius = circle_curve.radius();
            let side = axis.cross(*ref_direction);
            let point = center.translated(*ref_direction, radius * parameter.cos());
            Some(point.translated(side, radius * parameter.sin()))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center();
            let axis = ellipse_curve.axis();
            let major_direction = ellipse_curve.major_direction();
            let major_radius = ellipse_curve.major_radius();
            let minor_radius = ellipse_curve.minor_radius();
            let minor_direction = axis.cross(*major_direction);
            let point = center.translated(*major_direction, major_radius * parameter.cos());
            Some(point.translated(minor_direction, minor_radius * parameter.sin()))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = parabola_curve.vertex();
            let axis = parabola_curve.axis();
            let major_direction = parabola_curve.major_direction();
            let focal_distance = parabola_curve.focal_distance();
            let minor_direction = axis.cross(*major_direction);
            let point = vertex.translated(*major_direction, focal_distance * parameter * parameter);
            Some(point.translated(minor_direction, 2.0 * focal_distance * parameter))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            let center = hyperbola_curve.center();
            let axis = hyperbola_curve.axis();
            let major_direction = hyperbola_curve.major_direction();
            let major_radius = hyperbola_curve.major_radius();
            let minor_radius = hyperbola_curve.minor_radius();
            let minor_direction = axis.cross(*major_direction);
            let point = center.translated(*major_direction, major_radius * parameter.cosh());
            Some(point.translated(minor_direction, minor_radius * parameter.sinh()))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
            let point = degenerate_curve.point();
            Some(*point)
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let values = basis(
                nurbs.knots(),
                usize::try_from(nurbs.degree()).ok()?,
                nurbs.control_points().len(),
                parameter,
            )?;
            let mut point = Point3::new(0.0, 0.0, 0.0);
            let mut denominator = 0.0;
            for (index, value) in values.into_iter().enumerate() {
                let weight = nurbs.weights().map_or(1.0, |weights| weights[index]);
                let coefficient = value * weight;
                point.x += coefficient * nurbs.control_points()[index].x;
                point.y += coefficient * nurbs.control_points()[index].y;
                point.z += coefficient * nurbs.control_points()[index].z;
                denominator += coefficient;
            }
            (denominator != 0.0).then(|| {
                Point3::new(
                    point.x / denominator,
                    point.y / denominator,
                    point.z / denominator,
                )
            })
        }
        CurveGeometry::Solved(
            SolvedCurveGeometry::Polyline(_) | SolvedCurveGeometry::Transformed { .. },
        ) => cadmpeg_ir::eval::curve_point(geometry, parameter),
        CurveGeometry::Solved(
            SolvedCurveGeometry::Composite { .. } | SolvedCurveGeometry::Unknown { .. },
        )
        | CurveGeometry::Procedural { .. } => None,
    }
}

#[test]fn parabola_parameterization(){let g=PcurveGeometry::Parabola(ParabolaPcurve::try_new(Point2::new(0.,0.),Point2::new(1.,0.),Point2::new(0.,1.),2.).unwrap());let got=pcurve(&g,1.).unwrap();let neutral=cadmpeg_ir::eval::pcurve_uv(&g,1.).unwrap();println!("IGES parabola f=2 t=1: {got:?}; neutral evaluator={neutral:?}");assert_eq!(got,Point2::new(2.,4.));assert_eq!(neutral,Point2::new(0.125,1.));}
#[test]fn weighted_nurbs_overflow(){let n=PcurveNurbs::new(1,vec![0.,0.,1.,1.],PcurveNurbsPoles::from_lanes(vec![Point2::new(1e200,0.),Point2::new(2e200,0.)],Some(vec![1e200,1e200])).unwrap(),false).unwrap();let g=PcurveGeometry::Nurbs{nurbs:n};let got=pcurve(&g,0.5).unwrap();let neutral=cadmpeg_ir::eval::pcurve_uv(&g,0.5);println!("IGES rational pcurve={got:?}, neutral={neutral:?}");assert!(!got.is_finite());}
}
mod catia_axis {use super::*;fn unit_vector(v:Vector3)->Option<Vector3>{v.unit_nonzero()}fn circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<Vector3> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            close_length(center.vector_from(*origin).dot(*normal), 0.0).then_some(*normal)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.axis();
            let radius = cylinder_surface.radius();
            let offset = center.vector_from(*origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin();
            let axis = cone_surface.axis();
            let radius = cone_surface.radius();
            let half_angle = cone_surface.half_angle();
            let offset = center.vector_from(*origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let section_radius = (radius + axial * half_angle.tan()).abs();
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, section_radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let sphere_center = sphere_surface.center();
            let sphere_radius = sphere_surface.radius();
            let offset = center.vector_from(*sphere_center);
            let distance = offset.x.hypot(offset.y).hypot(offset.z);
            (distance.is_finite()
                && distance != 0.0
                && close_squared(
                    distance * distance + circle_radius * circle_radius,
                    sphere_radius * sphere_radius,
                ))
            .then(|| offset.scale(1.0 / distance))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let torus_center = torus_surface.center();
            let axis = torus_surface.axis();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
            let offset = center.vector_from(*torus_center);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let radial_distance = radial.norm();
            if close_length(axial, 0.0)
                && close_length(radial_distance, major_radius)
                && close_length(circle_radius, minor_radius)
            {
                unit_vector((*axis).cross(radial))
            } else if close_length(radial_distance, 0.0)
                && close_squared(
                    (circle_radius - major_radius).powi(2) + axial * axial,
                    minor_radius * minor_radius,
                )
            {
                Some(*axis)
            } else {
                None
            }
        }
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Nurbs(_)
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed { .. }
            | SolvedSurfaceGeometry::Unknown { .. },
        )
        | SurfaceGeometry::Procedural { .. } => None,
    }
}
fn close_length(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-5 * (1.0 + left.abs().max(right.abs()))
}
fn close_squared(left: f64, right: f64) -> bool {
    (left - right).abs() <= 2e-5 * (1.0 + left.abs().max(right.abs()))
}

#[test]fn disjoint_circle_and_sphere(){let sf=SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(cadmpeg_ir::geometry::analytic::SphereSurface::try_new(Point3::new(0.,0.,0.),Vector3::new(0.,0.,1.),Vector3::new(1.,0.,0.),1.).unwrap()));let got=circle_axis_from_carrier(Point3::new(1e200,0.,0.),1.,&sf);println!("CATIA sphere radius 1, circle radius 1 center=(1e200,0,0): axis={got:?}; expected None");assert!(got.is_some());}
}
mod more_expressions {use super::*;
#[test]fn conic_coefficient_scale(){for a in [1.,1e-200_f64]{let c=a;let f=-a;let ellipse=a*c>0.;let hyperbola=a*c<0.;println!("IGES common-scaled unit circle A=C={a:e}, F={f:e}: ellipse={ellipse} hyperbola={hyperbola}");assert_eq!(ellipse,a==1.);}}
#[test]fn rhino_reversed_trim_break(){let domain=[1e308_f64,1.4e308];let value=1.2e308;let got=domain[0]+domain[1]-value;let expected=cadmpeg_ir::math::reflect_parameter(value,domain[0],domain[1]).unwrap();println!("Rhino reversed trim knot={got}, robust={expected:e}");assert!(got.is_infinite());}
}
mod f3d_mesh {use super::*;use cadmpeg_core::CodecError;#[derive(Debug,Copy,Clone)]struct MeshAffineTransform([f64;16]);impl MeshAffineTransform {
    /// Check finite coefficients, an affine last row, and a nonzero finite determinant.
    pub(crate) fn new(cells: [f64; 16]) -> Result<Self, String> {
        let value = Self(cells);
        let rows = value.rows();
        if !cells.iter().all(|cell| cell.is_finite()) || rows[3] != [0.0, 0.0, 0.0, 1.0] {
            return Err("transform must be finite and affine".into());
        }
        let determinant = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
            - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
            + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
        if !determinant.is_finite() || determinant == 0.0 {
            return Err("transform must have a nonzero finite determinant".into());
        }
        Ok(value)
    }

    /// Row-major coefficients.
    pub(crate) fn cells(self) -> [f64; 16] {
        self.0
    }

    /// Four row-major rows.
    fn rows(self) -> [[f64; 4]; 4] {
        let cells = self.0;
        [
            [cells[0], cells[1], cells[2], cells[3]],
            [cells[4], cells[5], cells[6], cells[7]],
            [cells[8], cells[9], cells[10], cells[11]],
            [cells[12], cells[13], cells[14], cells[15]],
        ]
    }
}

impl MeshAffineTransform {fn transform_normal(self, normal: [f64; 3]) -> Result<cadmpeg_ir::math::Vector3, CodecError> {
        let cells = self.cells();
        let [x, y, z] = normal;
        let transformed = [
            (cells[5] * cells[10] - cells[6] * cells[9]) * x
                + (cells[6] * cells[8] - cells[4] * cells[10]) * y
                + (cells[4] * cells[9] - cells[5] * cells[8]) * z,
            (cells[2] * cells[9] - cells[1] * cells[10]) * x
                + (cells[0] * cells[10] - cells[2] * cells[8]) * y
                + (cells[1] * cells[8] - cells[0] * cells[9]) * z,
            (cells[1] * cells[6] - cells[2] * cells[5]) * x
                + (cells[2] * cells[4] - cells[0] * cells[6]) * y
                + (cells[0] * cells[5] - cells[1] * cells[4]) * z,
        ];
        let scale = transformed
            .iter()
            .map(|component| component.abs())
            .fold(0.0f64, f64::max);
        if !scale.is_finite() || scale == 0.0 {
            return Err(CodecError::Malformed(
                "F3D mesh placement produces a degenerate normal".into(),
            ));
        }
        let scaled = transformed.map(|component| component / scale);
        let length = (scaled[0] * scaled[0] + scaled[1] * scaled[1] + scaled[2] * scaled[2]).sqrt();
        if !length.is_finite() || length <= f64::EPSILON {
            return Err(CodecError::Malformed(
                "F3D mesh placement produces a degenerate normal".into(),
            ));
        }
        Ok(cadmpeg_ir::math::Vector3::new(
            scaled[0] / length,
            scaled[1] / length,
            scaled[2] / length,
        ))
    }
}
#[test]fn valid_anisotropic_normal(){let tr=MeshAffineTransform::new([1e200,0.,0.,0.,0.,1e200,0.,0.,0.,0.,1e-200,0.,0.,0.,0.,1.]).unwrap();let got=tr.transform_normal([0.,0.,1.]);println!("F3D admitted anisotropic transform, input Z normal: {got:?}; expected Z normal");assert!(got.is_err());}
}
mod asm {#[test]fn finite_translation_patch(){let header_scale=1e308_f64;let translation=1e308_f64;let len_to_mm=10.;let got=translation/(header_scale*len_to_mm);let expected=(translation/header_scale)/len_to_mm;println!("ASM native translation={got}, expected={expected}");assert_eq!(got,0.);assert_eq!(expected,0.1);}}
mod coordinate_scaling {use super::*;use cadmpeg_ir::geometry::pcurve::{PcurveGeometry,ParabolaPcurve};
#[test]fn parabola_parameter_is_not_preserved(){let mut g=PcurveGeometry::Parabola(ParabolaPcurve::try_new(Point2::new(0.,0.),Point2::new(1.,0.),Point2::new(0.,1.),2.).unwrap());let before=cadmpeg_ir::eval::pcurve_uv(&g,1.).unwrap();g.try_scale_coordinates([3.,3.]).unwrap();let after=cadmpeg_ir::eval::pcurve_uv(&g,1.).unwrap();let expected=Point2::new(3.*before.u,3.*before.v);println!("IR parabola coordinate scale 3 at fixed t=1: before={before:?}, after={after:?}, expected={expected:?}");assert_eq!(after,Point2::new(1./24.,1.));assert_eq!(expected,Point2::new(0.375,3.));assert_ne!(after,expected);}
}
