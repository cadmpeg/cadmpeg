// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted copies of construction operands.

use super::{ChamferGroup, ChamferSpec, FilletGroup, FullRoundFilletGroup, FullRoundSideSelection, RadiusForm, RadiusSpec, VariableRadii, VariableRadius};
use super::super::charged_copies::FeatureCopy;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

impl FeatureCopy for ChamferGroup {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { edges: self.edges.copy_feature(ctx, purpose)?, spec: self.spec.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for ChamferSpec {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FilletGroup {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { edges: self.edges.copy_feature(ctx, purpose)?, radius: self.radius.copy_feature(ctx, purpose)?, tangency_weight: self.tangency_weight.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for FullRoundFilletGroup {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { center: self.center.copy_feature(ctx, purpose)?, side_one: self.side_one.copy_feature(ctx, purpose)?, side_two: self.side_two.copy_feature(ctx, purpose)? })
    }
}

impl FeatureCopy for FullRoundSideSelection {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Automatic => Ok(Self::Automatic),
            Self::Explicit(value_0) => Ok(Self::Explicit(value_0.copy_feature(ctx, purpose)?)),
            Self::Unresolved => Ok(Self::Unresolved),
        }
    }
}

impl FeatureCopy for RadiusForm {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for RadiusSpec {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved { form } => Ok(Self::Unresolved { form: form.copy_feature(ctx, purpose)? }),
            Self::Constant { radius } => Ok(Self::Constant { radius: radius.copy_feature(ctx, purpose)? }),
            Self::Chordal { chord_length } => Ok(Self::Chordal { chord_length: chord_length.copy_feature(ctx, purpose)? }),
            Self::Asymmetric { offset_one, offset_two } => Ok(Self::Asymmetric { offset_one: offset_one.copy_feature(ctx, purpose)?, offset_two: offset_two.copy_feature(ctx, purpose)? }),
            Self::Variable { points } => Ok(Self::Variable { points: points.copy_feature(ctx, purpose)? }),
        }
    }
}

impl FeatureCopy for VariableRadii {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl<P: FeatureCopy, L: FeatureCopy> FeatureCopy for VariableRadius<P, L> {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { parameter: self.parameter.copy_feature(ctx, purpose)?, radius: self.radius.copy_feature(ctx, purpose)? })
    }
}

