// SPDX-License-Identifier: Apache-2.0
//! Quantized identity and checked integer-grid admission.

use cadmpeg_core::convert::{f64_from_i64, truncate_f64_to_i64};
use cadmpeg_ir::math::Point2;

/// `i64::MIN` as an `f64`; the value is exactly -2^63.
const I64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
/// `i64::MAX + 1` as an `f64`; the value is exactly 2^63.
const I64_END_F64: f64 = 9_223_372_036_854_775_808.0;

/// Outside i64 grid range, adjacent finite coordinates are farther apart than
/// one grid cell. Preserve their bits instead of saturating distinct cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum GridCoordinate {
    BelowRange(u64),
    Cell(i64),
    AboveRange(u64),
    Invalid(u64),
}

impl cadmpeg_core::decode::cost::DecodeCost for GridCoordinate {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl GridCoordinate {
    pub(super) fn new(value: f64, quantum: f64) -> Self {
        if !value.is_finite() || !quantum.is_finite() || quantum <= 0.0 {
            return Self::Invalid(value.to_bits());
        }
        let cell = (value / quantum).round();
        if cell < I64_MIN_F64 {
            Self::BelowRange(!value.to_bits())
        } else if cell >= I64_END_F64 {
            Self::AboveRange(value.to_bits())
        } else {
            match truncate_f64_to_i64(cell) {
                Some(cell) => Self::Cell(cell),
                None => Self::Invalid(value.to_bits()),
            }
        }
    }
    /// The coordinate this identity stands for, or `None` when its grid cell
    /// has no exact floating-point value.
    pub(super) fn coordinate(self, quantum: f64) -> Option<f64> {
        match self {
            Self::BelowRange(bits) => Some(f64::from_bits(!bits)),
            Self::Cell(cell) => Some(f64_from_i64(cell)? * quantum),
            Self::AboveRange(bits) | Self::Invalid(bits) => Some(f64::from_bits(bits)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct GridPoint(GridCoordinate, GridCoordinate);

impl cadmpeg_core::decode::cost::DecodeCost for GridPoint {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl GridPoint {
    pub(super) fn cells(self) -> Option<(i64, i64)> {
        match self {
            Self(GridCoordinate::Cell(u), GridCoordinate::Cell(v)) => Some((u, v)),
            _ => None,
        }
    }
    pub(super) fn point(self, quantum: f64) -> Option<Point2> {
        Some(Point2::new(
            self.0.coordinate(quantum)?,
            self.1.coordinate(quantum)?,
        ))
    }
}

impl From<(i64, i64)> for GridPoint {
    fn from((u, v): (i64, i64)) -> Self {
        Self(GridCoordinate::Cell(u), GridCoordinate::Cell(v))
    }
}

impl PartialEq<(i64, i64)> for GridPoint {
    fn eq(&self, other: &(i64, i64)) -> bool {
        self.cells().as_ref() == Some(other)
    }
}

pub(super) fn quantize(point: Point2, quantum: f64) -> GridPoint {
    GridPoint(
        GridCoordinate::new(point.u, quantum),
        GridCoordinate::new(point.v, quantum),
    )
}

#[cfg(test)]
mod tests {
    use super::{quantize, Point2};
    #[test]
    fn large_coordinate_keys_do_not_alias_or_enter_integer_transforms() {
        for sign in [-1., 1.] {
            let a = quantize(Point2::new(sign * 1e14, 0.), 1e-6);
            let b = quantize(Point2::new(sign * 2e14, 0.), 1e-6);
            assert_ne!(a, b);
            assert!(a.cells().is_none());
            assert_eq!(a.point(1e-6), Some(Point2::new(sign * 1e14, 0.)));
        }
        assert_eq!(
            quantize(Point2::new(1.25, -2.5), 0.25).cells(),
            Some((5, -10))
        );
        assert!(quantize(Point2::new(f64::NAN, 0.), 1.).cells().is_none());
    }
}
