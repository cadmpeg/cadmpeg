// SPDX-License-Identifier: Apache-2.0
//! Charged owned leaves and structural feature collections.

use super::{copy_identity, copy_sequence, FeatureCopy};
use super::super::{BodyMember, BodyMembers, DistinctMembers, NonEmptyMembers, SelectionMembers};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

macro_rules! copy_feature_scalars {
    ($($value:ty),+ $(,)?) => { $(
        impl FeatureCopy for $value {
            fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
                ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
                Ok(*self)
            }
        }
    )+ };
}

copy_feature_scalars!(
bool,
u32,
u64,
f64,
std::num::NonZeroU32,
crate::topology::IncreasingParameterInterval,
crate::transform::Transform,
crate::units::UnitVector3,
crate::scalar::Angle,
crate::scalar::FiniteReal,
crate::scalar::Fraction,
crate::scalar::InteriorAngle,
crate::scalar::Length,
crate::scalar::NonNegativeLength,
crate::scalar::NonZeroLength,
crate::scalar::PositiveAngle,
crate::scalar::PositiveLength,
crate::scalar::PositiveReal,
crate::scalar::SlopeAngle,
super::super::FeatureDatumPlaneFrame,
super::super::FeatureDirection3,
super::super::FeatureRigidPlacement,
super::super::FeatureSupportPlaneFrame,
super::super::FinitePoint3,
super::super::FiniteVector3
);

macro_rules! copy_feature_identities {
    ($($value:ty),+ $(,)?) => { $(
        impl FeatureCopy for $value {
            fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
                copy_identity(ctx, self.as_str(), purpose, Self::mint)
            }
        }
    )+ };
}

copy_feature_identities!(
crate::assets::AssetId,
crate::products::JointId,
crate::ids::BodyId,
crate::ids::CurveId,
crate::ids::EdgeId,
crate::ids::FeatureInputTopologyId,
crate::ids::HistoricalBodyId,
crate::ids::HistoricalEdgeId,
crate::ids::OccurrenceId,
crate::ids::SubdId,
crate::ids::VertexId,
super::super::FeatureId,
super::super::ParameterId,
crate::sketches::SketchEntityId,
crate::sketches::SketchId,
crate::sketches::SpatialSketchEntityId,
crate::sketches::SpatialSketchId
);

impl FeatureCopy for String {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        super::super::copy_feature_selection_text(ctx, self, purpose)
    }
}

impl FeatureCopy for NonBlankString {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        NonBlankString::new(super::super::copy_feature_selection_text(ctx, self.as_str(), purpose)?)
            .ok_or_else(|| CodecError::malformed("invalid admitted feature text"))
    }
}

impl<T: FeatureCopy> FeatureCopy for Option<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        self.as_ref().map(|value| value.copy_feature(ctx, purpose)).transpose()
    }
}

impl<T: FeatureCopy> FeatureCopy for Vec<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        copy_sequence(ctx, self, purpose, |value| value.copy_feature(ctx, purpose))
    }
}

impl<T: FeatureCopy> FeatureCopy for Box<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<T>()), purpose)?;
        ctx.charge_collection_items(1, purpose)?;
        Ok(Box::new(self.as_ref().copy_feature(ctx, purpose)?))
    }
}

impl<T: FeatureCopy, const N: usize> FeatureCopy for [T; N] {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        let copied = copy_sequence(ctx, self, purpose, |value| value.copy_feature(ctx, purpose))?;
        copied.try_into().map_err(|_| CodecError::malformed("invalid copied feature array length"))
    }
}

impl<T: FeatureCopy> FeatureCopy for DistinctMembers<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl<T: FeatureCopy> FeatureCopy for NonEmptyMembers<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl<T: FeatureCopy> FeatureCopy for SelectionMembers<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl<T: FeatureCopy> FeatureCopy for BodyMember<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { body: self.body.copy_feature(ctx, purpose)?, native: self.native.copy_feature(ctx, purpose)? })
    }
}

impl<T: FeatureCopy> FeatureCopy for BodyMembers<T> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for BTreeMap<NonBlankString, String> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(self.len()), purpose)?;
        let bytes = self.keys().try_fold(0u64, |bytes, key| bytes.checked_add(u64_from_index(key.as_str().len())))
            .ok_or_else(|| ctx.refuse_codec_limit(purpose, u64::MAX - 1, u64::MAX))?;
        let mut copied = BTreeMap::new();
        for (key, value) in self {
            ctx.charge_work(bytes.checked_add(u64_from_index(key.as_str().len())).and_then(|bytes| bytes.checked_mul(8))
                .and_then(|work| work.checked_add(u64_from_index(self.len()).checked_mul(64)?))
                .ok_or_else(|| ctx.refuse_codec_limit(purpose, u64::MAX - 1, u64::MAX))?, purpose)?;
            ctx.charge_collection_items(1, purpose)?;
            copied.insert(key.copy_feature(ctx, purpose)?, value.copy_feature(ctx, purpose)?);
        }
        Ok(copied)
    }
}
