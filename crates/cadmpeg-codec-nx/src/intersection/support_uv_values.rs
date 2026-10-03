// SPDX-License-Identifier: Apache-2.0
//! Finite support-UV tuples with their exact packing marker.

use super::{SupportUv, SupportUvLane};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;

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
    fn with_storage(
        packing: SupportUvPacking,
        values: Vec<f64>,
        mut finite: Vec<FiniteReal>,
    ) -> Result<Self, &'static str> {
        let count = u32::try_from(values.len()).map_err(|_| "values: scalar count exceeds u32")?;
        if values.is_empty() || !values.len().is_multiple_of(packing.width()) {
            return Err("values: must contain nonempty complete tuples for marker");
        }
        if !finite.is_empty() || finite.capacity() < values.len() {
            return Err("values: admitted storage is too small or not empty");
        }
        for value in values {
            finite.push(FiniteReal::new(value).ok_or("values: scalars must be finite")?);
        }
        Ok(Self {
            packing,
            values: finite,
            count,
        })
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        packing: SupportUvPacking,
        values: Vec<f64>,
    ) -> Result<Option<Self>, CodecError> {
        ctx.charge_work(u64_from_index(values.len()), "admit NX support-UV scalars")?;
        let (finite, reservation) =
            ctx.temporary_vec(values.len(), "NX finite support-UV values")?;
        let data = Self::with_storage(packing, values, finite).ok();
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
        Self::with_storage(packing, values, finite)
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
        let count_u64 = u64_from_index(count);
        let lane_count = if self.packing == SupportUvPacking::Form4 {
            2
        } else {
            1
        };
        ctx.charge_collection_items(
            count_u64
                .checked_mul(lane_count)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, count_u64))?,
            operation,
        )?;
        let mut first = Vec::new();
        ctx.reserve_capacity(&mut first, count, operation)?;
        let mut second = if lane_count == 2 {
            let mut lane = Vec::new();
            ctx.reserve_capacity(&mut lane, count, operation)?;
            Some(lane)
        } else {
            None
        };
        ctx.charge_work(count_u64, "project NX support-UV tuples")?;
        for row in self.values.chunks_exact(self.packing.width()) {
            first.push(FiniteVector::from([row[0], row[1]]));
            if let Some(values) = &mut second {
                values.push(FiniteVector::from([row[2], row[3]]));
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
}
