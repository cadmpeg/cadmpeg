// SPDX-License-Identifier: Apache-2.0
//! Count-selected physical LIMIT storage (PS-17).

use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use serde::{Deserialize, Serialize};

use super::{ChartPointLayout, TermUseForm};

/// One or two physical rows, with no empty or oversized state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum LimitRows<T> {
    One([T; 1]),
    Two([T; 2]),
}

impl<T> LimitRows<T> {
    pub(crate) fn as_slice(&self) -> &[T] {
        match self {
            Self::One(rows) => rows,
            Self::Two(rows) => rows,
        }
    }

    pub(crate) fn count(&self) -> u32 {
        match self {
            Self::One(_) => 1,
            Self::Two(_) => 2,
        }
    }
}

/// A finite unit tangent without changing its serialized components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FiniteVector<3>", into = "FiniteVector<3>")]
pub(crate) struct LimitTangent(FiniteVector<3>);

impl TryFrom<FiniteVector<3>> for LimitTangent {
    type Error = &'static str;
    fn try_from(value: FiniteVector<3>) -> Result<Self, Self::Error> {
        let [x, y, z] = value.get();
        ((x.hypot(y).hypot(z) - 1.0).abs() < super::EPS_INTERSECTION_CHART_POINTS_E9)
            .then_some(Self(value))
            .ok_or("LIMIT tangent must have unit length")
    }
}
impl From<LimitTangent> for FiniteVector<3> {
    fn from(value: LimitTangent) -> Self {
        value.0
    }
}
impl LimitTangent {
    #[cfg(test)]
    fn get(self) -> [f64; 3] {
        self.0.get()
    }
}

/// Complete NX extension of an hvec. Only its position is a model length.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct Hvec {
    pub(crate) point: FiniteVector<3>,
    pub(crate) u: [FiniteReal; 2],
    pub(crate) v: [FiniteReal; 2],
    pub(crate) tangent: LimitTangent,
    pub(crate) parameter: FiniteReal,
}

impl Hvec {
    fn read(stream: &[u8], at: usize) -> Option<Self> {
        let point = super::point_m(stream, at)?;
        let mut view =
            View::over_retained(stream).child(at.checked_add(24)?, at.checked_add(88)?)?;
        let u = [
            FiniteReal::new(view.f64_be()?)?,
            FiniteReal::new(view.f64_be()?)?,
        ];
        let v = [
            FiniteReal::new(view.f64_be()?)?,
            FiniteReal::new(view.f64_be()?)?,
        ];
        let tangent = LimitTangent::try_from(FiniteVector::new([
            view.f64_be()?,
            view.f64_be()?,
            view.f64_be()?,
        ])?)
        .ok()?;
        let parameter = FiniteReal::new(view.f64_be()?)?;
        Some(Self {
            point,
            u,
            v,
            tangent,
            parameter,
        })
    }
}

/// Complete physical storage; the form separately fixes its row count.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "layout", content = "hvecs", rename_all = "snake_case")]
pub(crate) enum LimitState {
    Xyz3(LimitRows<FiniteVector<3>>),
    Ext11(LimitRows<Hvec>),
}

impl LimitState {
    pub(crate) fn count(&self) -> u32 {
        match self {
            Self::Xyz3(rows) => rows.count(),
            Self::Ext11(rows) => rows.count(),
        }
    }

    /// The exact hard limit or singularity, never the nearby branch selector.
    pub(crate) fn point(&self) -> FiniteVector<3> {
        match self {
            Self::Xyz3(rows) => rows.as_slice()[0],
            Self::Ext11(rows) => rows.as_slice()[0].point,
        }
    }

    pub(crate) fn read(
        stream: &[u8],
        at: usize,
        form: TermUseForm,
        layout: ChartPointLayout,
    ) -> Option<(Self, usize)> {
        let count = form.count();
        let width = match layout {
            ChartPointLayout::Xyz3 => 24,
            ChartPointLayout::Ext11 => 88,
        };
        let end = at.checked_add(width * usize::try_from(count).ok()?)?;
        stream.get(at..end)?;
        let state = match layout {
            ChartPointLayout::Xyz3 => {
                let first = super::point_m(stream, at)?;
                Self::Xyz3(if count == 1 {
                    LimitRows::One([first])
                } else {
                    LimitRows::Two([first, super::point_m(stream, at + width)?])
                })
            }
            ChartPointLayout::Ext11 => {
                let first = Hvec::read(stream, at)?;
                Self::Ext11(if count == 1 {
                    LimitRows::One([first])
                } else {
                    LimitRows::Two([first, Hvec::read(stream, at + width)?])
                })
            }
        };
        Some((state, end))
    }
}

#[cfg(test)]
mod tests {
    use super::{ChartPointLayout, LimitRows, LimitState, TermUseForm};

    #[test]
    fn complete_hvec_preserves_parameter_roles_and_scales_only_position() {
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 1.0, 0.0, 0.0, 8.0];
        let bytes: Vec<_> = values.into_iter().flat_map(f64::to_be_bytes).collect();
        let (state, end) =
            LimitState::read(&bytes, 0, TermUseForm::LQuestion, ChartPointLayout::Ext11).unwrap();
        assert_eq!(end, 88);
        let LimitState::Ext11(LimitRows::One([hvec])) = state else {
            panic!("one complete hvec")
        };
        assert_eq!(hvec.point.get(), [1000.0, 2000.0, 3000.0]);
        assert_eq!(hvec.u.map(super::FiniteReal::get), [4.0, 5.0]);
        assert_eq!(hvec.v.map(super::FiniteReal::get), [6.0, 7.0]);
        assert_eq!(hvec.tangent.get(), [1.0, 0.0, 0.0]);
        assert_eq!(hvec.parameter.get(), 8.0);
        assert!(LimitState::read(
            &bytes[..87],
            0,
            TermUseForm::LQuestion,
            ChartPointLayout::Ext11
        )
        .is_none());
    }

    #[test]
    fn partition_terminator_requires_both_stored_positions() {
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0];
        let bytes: Vec<_> = values.into_iter().flat_map(f64::to_be_bytes).collect();
        let (state, end) =
            LimitState::read(&bytes, 0, TermUseForm::Tf, ChartPointLayout::Xyz3).unwrap();
        assert_eq!(end, 48);
        assert_eq!(state.count(), 2);
        assert_eq!(state.point().get(), [1000.0, 2000.0, 3000.0]);
        assert!(
            LimitState::read(&bytes[..47], 0, TermUseForm::Tf, ChartPointLayout::Xyz3).is_none()
        );
        assert!(serde_json::from_str::<super::LimitRows<u32>>("[]").is_err());
        assert!(serde_json::from_str::<super::LimitRows<u32>>("[1,2,3]").is_err());
    }
    #[test]
    fn full_hvec_storage_rejects_each_nonfinite_scalar_and_preserves_signed_zero() {
        let values = [
            -0.0_f64,
            0.0,
            0.0,
            super::super::MISSING_PARAMETER,
            -0.0,
            0.0,
            0.0,
            1.0,
            -0.0,
            0.0,
            -0.0,
        ];
        let mut bytes: Vec<_> = values
            .into_iter()
            .cycle()
            .take(22)
            .flat_map(f64::to_be_bytes)
            .collect();
        let (state, end) =
            LimitState::read(&bytes, 0, TermUseForm::Tf, ChartPointLayout::Ext11).unwrap();
        assert_eq!(end, 176);
        let LimitState::Ext11(LimitRows::Two(rows)) = state else {
            panic!("two complete hvecs")
        };
        for row in rows {
            assert_eq!(row.point.get()[0].to_bits(), (-0.0_f64).to_bits());
            assert_eq!(row.u[0].get(), super::super::MISSING_PARAMETER);
            assert_eq!(row.u[1].get().to_bits(), (-0.0_f64).to_bits());
            assert_eq!(row.tangent.get()[1].to_bits(), (-0.0_f64).to_bits());
            assert_eq!(row.parameter.get().to_bits(), (-0.0_f64).to_bits());
        }
        for scalar in 0..22 {
            let start = scalar * 8;
            let saved: [u8; 8] = bytes[start..start + 8].try_into().unwrap();
            for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                bytes[start..start + 8].copy_from_slice(&invalid.to_be_bytes());
                assert!(
                    LimitState::read(&bytes, 0, TermUseForm::Tf, ChartPointLayout::Ext11).is_none(),
                    "scalar {scalar}"
                );
            }
            bytes[start..start + 8].copy_from_slice(&saved);
        }
    }
}
