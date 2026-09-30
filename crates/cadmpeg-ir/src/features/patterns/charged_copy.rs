// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted copies of construction operands.

use super::{CompositePattern, LinearPatternDirection, NoNestedComposite, PatternForm, PatternKind, PatternScaleCenter, PatternSeed, PatternStage, PatternTransform};
use super::super::charged_copies::FeatureCopy;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

impl FeatureCopy for CompositePattern {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for LinearPatternDirection {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { direction: self.direction.copy_feature(ctx, purpose)?, spacing: self.spacing.copy_feature(ctx, purpose)?, count: self.count.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for NoNestedComposite {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for PatternForm {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl<C: FeatureCopy> FeatureCopy for PatternKind<C> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for PatternScaleCenter {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::FirstSeedCentroid => Ok(Self::FirstSeedCentroid),
            Self::Point(value_0) => Ok(Self::Point(value_0.copy_feature(ctx, purpose)?)),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for PatternSeed {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Feature(value_0) => Ok(Self::Feature(value_0.copy_feature(ctx, purpose)?)),
            Self::Faces(value_0) => Ok(Self::Faces(value_0.copy_feature(ctx, purpose)?)),
            Self::Bodies(value_0) => Ok(Self::Bodies(value_0.copy_feature(ctx, purpose)?)),
            Self::Occurrences(value_0) => Ok(Self::Occurrences(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for PatternStage {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { pattern: self.pattern.copy_feature(ctx, purpose)? })
    }
}

impl<C: FeatureCopy> FeatureCopy for PatternTransform<C> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved { form } => Ok(Self::Unresolved { form: form.copy_feature(ctx, purpose)? }),
            Self::Linear { direction, spacing, count, second } => Ok(Self::Linear { direction: direction.copy_feature(ctx, purpose)?, spacing: spacing.copy_feature(ctx, purpose)?, count: count.copy_feature(ctx, purpose)?, second: second.copy_feature(ctx, purpose)? }),
            Self::LinearOffsets { direction, offsets } => Ok(Self::LinearOffsets { direction: direction.copy_feature(ctx, purpose)?, offsets: offsets.copy_feature(ctx, purpose)? }),
            Self::Circular { axis_origin, axis_dir, angle, count } => Ok(Self::Circular { axis_origin: axis_origin.copy_feature(ctx, purpose)?, axis_dir: axis_dir.copy_feature(ctx, purpose)?, angle: angle.copy_feature(ctx, purpose)?, count: count.copy_feature(ctx, purpose)? }),
            Self::CircularAngles { axis_origin, axis_dir, angles } => Ok(Self::CircularAngles { axis_origin: axis_origin.copy_feature(ctx, purpose)?, axis_dir: axis_dir.copy_feature(ctx, purpose)?, angles: angles.copy_feature(ctx, purpose)? }),
            Self::CurveDriven { path, spacing, count } => Ok(Self::CurveDriven { path: path.copy_feature(ctx, purpose)?, spacing: spacing.copy_feature(ctx, purpose)?, count: count.copy_feature(ctx, purpose)? }),
            Self::Mirror { plane_origin, plane_normal } => Ok(Self::Mirror { plane_origin: plane_origin.copy_feature(ctx, purpose)?, plane_normal: plane_normal.copy_feature(ctx, purpose)? }),
            Self::MirrorReference { plane } => Ok(Self::MirrorReference { plane: plane.copy_feature(ctx, purpose)? }),
            Self::Scale { center, final_factor, count } => Ok(Self::Scale { center: center.copy_feature(ctx, purpose)?, final_factor: final_factor.copy_feature(ctx, purpose)?, count: count.copy_feature(ctx, purpose)? }),
            Self::Composite { stages } => Ok(Self::Composite { stages: stages.copy_feature(ctx, purpose)? }),
        }
    }
}

