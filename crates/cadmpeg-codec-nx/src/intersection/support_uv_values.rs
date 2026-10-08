// SPDX-License-Identifier: Apache-2.0
//! Finite support-UV tuples with their exact packing marker.

use super::{SupportUv, SupportUvLane};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use std::convert::Infallible;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportUvPacking {
    Form2,
    Form3,
    Form4,
}

impl TryFrom<u8> for SupportUvPacking {
    type Error = &'static str;
    fn try_from(marker: u8) -> Result<Self, Self::Error> {
        match marker {
            2 => Ok(Self::Form2),
            3 => Ok(Self::Form3),
            4 => Ok(Self::Form4),
            _ => Err("marker: must be 2, 3, or 4"),
        }
    }
}
impl SupportUvPacking {
    fn marker(self) -> u8 {
        match self {
            Self::Form2 => 2,
            Self::Form3 => 3,
            Self::Form4 => 4,
        }
    }
    fn width(self) -> usize {
        match self {
            Self::Form2 | Self::Form3 => 2,
            Self::Form4 => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SupportUvValues {
    packing: SupportUvPacking,
    values: Vec<FiniteReal>,
    count: u32,
}
impl SupportUvValues {
    fn with_storage<E>(
        packing: SupportUvPacking,
        values: &[f64],
        mut finite: Vec<FiniteReal>,
        mut next: impl FnMut(&mut std::slice::Iter<'_, f64>) -> Result<Option<f64>, E>,
        mut push: impl FnMut(&mut Vec<FiniteReal>, FiniteReal) -> Result<(), E>,
    ) -> Result<Result<Self, &'static str>, E> {
        let Ok(count) = u32::try_from(values.len()) else {
            return Ok(Err("values: scalar count exceeds u32"));
        };
        if values.is_empty() || !values.len().is_multiple_of(packing.width()) {
            return Ok(Err(
                "values: must contain nonempty complete tuples for marker",
            ));
        }
        let mut values = values.iter();
        while let Some(value) = next(&mut values)? {
            let Some(value) = FiniteReal::new(value) else {
                return Ok(Err("values: scalars must be finite"));
            };
            push(&mut finite, value)?;
        }
        Ok(Ok(Self {
            packing,
            values: finite,
            count,
        }))
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        packing: SupportUvPacking,
        values: Vec<f64>,
    ) -> Result<Option<Self>, CodecError> {
        let mut reservation = ctx.reserve_scoped(0, "NX finite support-UV values")?;
        let data = Self::with_storage(
            packing,
            &values,
            Vec::new(),
            |values| {
                Ok(ctx
                    .next_charged(values, "admit NX support-UV scalars")?
                    .copied())
            },
            |finite, value| {
                reservation
                    .with_storage(|| ctx.push_vec(finite, value, "NX finite support-UV values"))
            },
        )?
        .ok();
        if data.is_some() {
            reservation.commit()?;
        }
        Ok(data)
    }

    pub(crate) fn new(packing: SupportUvPacking, values: Vec<f64>) -> Result<Self, &'static str> {
        let finite = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(values.len()).map(|()| storage)
        }
        .map_err(|_| "values: storage allocation failed")?;
        match Self::with_storage(
            packing,
            &values,
            finite,
            |values| Ok::<_, Infallible>(values.next().copied()),
            |finite, value| {
                finite.push(value);
                Ok(())
            },
        ) {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }

    pub(super) fn copy_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            packing: self.packing,
            values: ctx.copy_slice(&self.values, operation)?,
            count: self.count,
        })
    }

    pub(crate) fn count(&self) -> u32 {
        self.count
    }
    pub(crate) fn marker(&self) -> u8 {
        self.packing.marker()
    }
    pub(super) fn packing(&self) -> SupportUvPacking {
        self.packing
    }
    pub(crate) fn values(&self) -> &[FiniteReal] {
        &self.values
    }
    #[cfg(test)]
    pub(crate) fn into_values(self) -> Vec<f64> {
        self.values.into_iter().map(FiniteReal::get).collect()
    }

    #[cfg(test)]
    pub(super) fn support_uv(&self, sample_count: usize) -> SupportUv {
        if sample_count < 2 {
            return [None, None];
        }
        let first = self
            .values()
            .chunks_exact(self.packing.width())
            .map(|row| FiniteVector::from([row[0], row[1]]))
            .collect();
        let second = match self.packing {
            SupportUvPacking::Form2 | SupportUvPacking::Form3 => None,
            SupportUvPacking::Form4 => Some(
                self.values()
                    .chunks_exact(4)
                    .map(|row| FiniteVector::from([row[2], row[3]]))
                    .collect(),
            ),
        };
        [
            SupportUvLane::from_checked(first, sample_count),
            second.and_then(|values| SupportUvLane::from_checked(values, sample_count)),
        ]
    }

    pub(super) fn support_uv_charged(
        &self,
        ctx: &DecodeContext<'_>,
        sample_count: usize,
    ) -> Result<SupportUv, CodecError> {
        let count = self.values.len() / self.packing.width();
        if sample_count < 2 || count != sample_count {
            return Ok([None, None]);
        }
        let operation = "NX solved support-UV values";
        let mut first = ctx.collection_vec(count, operation)?;
        let mut second = if self.packing == SupportUvPacking::Form4 {
            Some(ctx.collection_vec(count, operation)?)
        } else {
            None
        };
        for index in ctx.admit_iter(0..count, "project NX support-UV tuples")? {
            let at = index * self.packing.width();
            // Complete tuple validation fixes each row's width at two or four.
            first.push(FiniteVector::from([self.values[at], self.values[at + 1]]));
            if let Some(values) = &mut second {
                values.push(FiniteVector::from([
                    self.values[at + 2],
                    self.values[at + 3],
                ]));
            }
        }
        Ok([
            SupportUvLane::from_checked(first, sample_count),
            second.and_then(|values| SupportUvLane::from_checked(values, sample_count)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::{SupportUvPacking, SupportUvValues};

    #[test]
    fn packing_requires_complete_finite_tuples() {
        for marker in [2, 3, 4] {
            let packing = SupportUvPacking::try_from(marker).unwrap();
            let width = packing.width();
            let values = SupportUvValues::new(
                packing,
                std::iter::repeat_n(0.0, width * 2).collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(values.marker(), marker);
            assert_eq!(values.support_uv(2)[1].is_some(), marker == 4);
            for len in [0, width * 2 + 1] {
                assert!(SupportUvValues::new(
                    packing,
                    std::iter::repeat_n(0.0, len).collect::<Vec<_>>()
                )
                .is_err());
            }
            let mut values = std::iter::repeat_n(0.0, width * 2).collect::<Vec<_>>();
            values[0] = f64::NAN;
            assert!(SupportUvValues::new(packing, values).is_err());
        }
        assert!(SupportUvPacking::try_from(1).is_err());
    }
}

#[cfg(test)]
mod constructor_tests {
    use super::{SupportUvPacking, SupportUvValues};

    #[test]
    fn packed_uv_decode_and_wire_constructors_share_invariants() {
        for packing in [
            SupportUvPacking::Form2,
            SupportUvPacking::Form3,
            SupportUvPacking::Form4,
        ] {
            for values in [
                vec![],
                vec![0.0; 4],
                vec![0.0; 8],
                vec![0.0; 5],
                vec![f64::INFINITY; 8],
            ] {
                crate::test_support::with_decode_context(|ctx| {
                    assert_eq!(
                        SupportUvValues::new_charged(ctx, packing, values.clone()).unwrap(),
                        SupportUvValues::new(packing, values).ok()
                    );
                });
            }
        }
    }
}

#[cfg(test)]
mod physical_lane_tests {
    use super::{SupportUvPacking, SupportUvValues};

    #[test]
    fn single_support_uv_tuple_survives_without_chart_projection() {
        for marker in [2, 3, 4] {
            let packing = SupportUvPacking::try_from(marker).unwrap();
            crate::test_support::with_decode_context(|ctx| {
                let scalars = vec![0.0; packing.width()];
                let values = SupportUvValues::new_charged(ctx, packing, scalars.clone())
                    .unwrap()
                    .unwrap();
                assert_eq!(values.count(), u32::try_from(scalars.len()).unwrap());
                assert_eq!(values.marker(), marker);
                assert_eq!(values.into_values(), scalars);
                let values = SupportUvValues::new(packing, scalars).unwrap();
                assert_eq!(values.support_uv_charged(ctx, 1).unwrap(), [None, None]);
                assert_eq!(values.support_uv_charged(ctx, 2).unwrap(), [None, None]);
            });
        }
    }
    #[test]
    fn support_uv_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "admit NX support-UV scalars",
            |ctx| SupportUvValues::new_charged(ctx, SupportUvPacking::Form2, vec![0.0, 0.0]),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "admit NX support-UV scalars"));
    }
}

#[cfg(test)]
mod validation_budget_tests {
    use super::{SupportUvPacking, SupportUvValues};

    #[test]
    fn support_uv_validation_does_not_visit_an_invalid_scalars_suffix() {
        let mut values = vec![0.0; 4096];
        values[0] = f64::INFINITY;
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 1;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                assert!(
                    SupportUvValues::new_charged(ctx, SupportUvPacking::Form2, values)
                        .unwrap()
                        .is_none()
                )
            },
        );
    }
}
