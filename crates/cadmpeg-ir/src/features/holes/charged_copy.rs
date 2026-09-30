// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted copies of construction operands.

use super::{HoleBottom, HoleConstruction, HoleKind, HolePlacement, HoleProfileFilter, HoleShape};
use super::super::charged_copies::FeatureCopy;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

impl FeatureCopy for HoleBottom {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for HoleConstruction {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        self.try_clone_charged(ctx, purpose)
    }
}

impl FeatureCopy for HoleKind {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for HolePlacement {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Directed { position, direction } => Ok(Self::Directed { position: position.copy_feature(ctx, purpose)?, direction: direction.copy_feature(ctx, purpose)? }),
            Self::Axis { origin, axis } => Ok(Self::Axis { origin: origin.copy_feature(ctx, purpose)?, axis: axis.copy_feature(ctx, purpose)? }),
        }
    }
}

impl FeatureCopy for HoleProfileFilter {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for HoleShape {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, purpose: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self { construction: self.construction.copy_feature(ctx, purpose)?, exit_kind: self.exit_kind.copy_feature(ctx, purpose)?, diameter: self.diameter.copy_feature(ctx, purpose)? })
    }
}

