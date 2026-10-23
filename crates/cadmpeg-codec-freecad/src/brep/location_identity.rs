// SPDX-License-Identifier: Apache-2.0
//! Exact location identity independent of affine arithmetic.

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

use super::{location_transform_error, transform_power, Tables, TextLocation};

const MAX_LOCATION_TERMS: usize = 65_536;
const MAX_LOCATION_WORK: usize = 16_777_216;

/// A local canonicalization bound or a decode failure.
#[derive(Debug)]
pub(crate) enum LocationIdentityError {
    Bounded,
    Decode(CodecError),
}

impl From<CodecError> for LocationIdentityError {
    fn from(error: CodecError) -> Self {
        Self::Decode(error)
    }
}

/// Ordered elementary datum identities with signed composition powers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LocationIdentity(Vec<(usize, i64)>);

impl LocationIdentity {
    pub(crate) fn identity() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn term_count(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn transform(&self, locations: &[TextLocation]) -> Result<Transform, CodecError> {
        let mut result = Transform::identity();
        for &(datum, power) in &self.0 {
            let location = datum
                .checked_sub(1)
                .and_then(|index| locations.get(index))
                .ok_or_else(|| CodecError::Malformed("missing elementary location datum".into()))?;
            let powered = transform_power(location.transform, power)?;
            result = result.compose(powered).map_err(location_transform_error)?;
        }
        Ok(result)
    }

    pub(crate) fn compose(
        &self,
        ctx: &DecodeContext<'_>,
        other: &Self,
    ) -> Result<Self, LocationIdentityError> {
        self.compose_bounded(ctx, other, &WorkBudget::new(MAX_LOCATION_WORK))
    }

    fn compose_bounded(
        &self,
        ctx: &DecodeContext<'_>,
        other: &Self,
        work: &WorkBudget,
    ) -> Result<Self, LocationIdentityError> {
        let count = self
            .0
            .len()
            .checked_add(other.0.len())
            .ok_or(LocationIdentityError::Bounded)?;
        charge(work, count)?;
        let mut terms = ctx.alloc_filled(count, (0_usize, 0_i64), "freecad_location_identity")?;
        let mut used = 0;
        for &(datum, power) in self.0.iter().chain(&other.0) {
            if used > 0 && terms[used - 1].0 == datum {
                let sum = terms[used - 1]
                    .1
                    .checked_add(power)
                    .ok_or(LocationIdentityError::Bounded)?;
                if sum == 0 {
                    used -= 1;
                } else {
                    terms[used - 1].1 = sum;
                }
            } else {
                if used == MAX_LOCATION_TERMS {
                    return Err(LocationIdentityError::Bounded);
                }
                terms[used] = (datum, power);
                used += 1;
            }
        }
        terms.truncate(used);
        Ok(Self(terms))
    }

    fn power(
        &self,
        ctx: &DecodeContext<'_>,
        power: i64,
        work: &WorkBudget,
    ) -> Result<Self, LocationIdentityError> {
        if power == 0 || self.0.is_empty() {
            return Ok(Self::identity());
        }
        if let [(datum, exponent)] = self.0.as_slice() {
            charge(work, 1)?;
            let exponent = exponent
                .checked_mul(power)
                .ok_or(LocationIdentityError::Bounded)?;
            return Ok(Self(ctx.alloc_filled(
                1,
                (*datum, exponent),
                "freecad_location_power",
            )?));
        }
        charge(work, self.0.len())?;
        let mut factor = ctx.alloc_filled(self.0.len(), (0, 0), "freecad_location_power")?;
        for (target, &(datum, exponent)) in factor.iter_mut().zip(&self.0) {
            *target = (
                datum,
                if power < 0 {
                    exponent
                        .checked_neg()
                        .ok_or(LocationIdentityError::Bounded)?
                } else {
                    exponent
                },
            );
        }
        if power < 0 {
            factor.reverse();
        }
        let mut factor = Self(factor);
        let mut result = Self::identity();
        let mut remaining = power.unsigned_abs();
        while remaining != 0 {
            if remaining & 1 != 0 {
                result = result.compose_bounded(ctx, &factor, work)?;
            }
            remaining >>= 1;
            if remaining != 0 {
                factor = factor.compose_bounded(ctx, &factor, work)?;
            }
        }
        Ok(result)
    }
}

impl Tables<'_> {
    pub(crate) fn location_identities(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<LocationIdentity>, LocationIdentityError> {
        identities(ctx, self.locations)
    }
}

fn identities(
    ctx: &DecodeContext<'_>,
    locations: &[TextLocation],
) -> Result<Vec<LocationIdentity>, LocationIdentityError> {
    let allowance = locations
        .len()
        .checked_mul(1024)
        .ok_or(LocationIdentityError::Bounded)?
        .max(MAX_LOCATION_WORK);
    let work = WorkBudget::new(allowance);
    let count = locations
        .len()
        .checked_add(1)
        .ok_or(LocationIdentityError::Bounded)?;
    charge(&work, count)?;
    let mut result = ctx.alloc_filled(count, LocationIdentity::identity(), "freecad_locations")?;
    for (position, location) in locations.iter().enumerate() {
        let index = position + 1;
        if location.factors.is_empty() {
            result[index] =
                LocationIdentity(ctx.alloc_filled(1, (index, 1), "freecad_location_datum")?);
        } else {
            let mut identity = LocationIdentity::identity();
            for factor in &location.factors {
                charge(&work, 1)?;
                if factor.location == 0 || factor.location >= index {
                    return Err(CodecError::Malformed(
                        "location factor must refer to an earlier nonzero location".into(),
                    )
                    .into());
                }
                let powered = result[factor.location].power(ctx, factor.power, &work)?;
                identity = powered.compose_bounded(ctx, &identity, &work)?;
            }
            result[index] = identity;
        }
    }
    Ok(result)
}

fn charge(work: &WorkBudget, count: usize) -> Result<(), LocationIdentityError> {
    if work.charge_by(count) {
        Ok(())
    } else {
        Err(LocationIdentityError::Bounded)
    }
}

#[cfg(test)]
mod tests;
