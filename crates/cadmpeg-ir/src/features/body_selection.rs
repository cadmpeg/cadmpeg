// SPDX-License-Identifier: Apache-2.0
//! Admission of native, historical and generated body selections.

use super::{
    BodySelection, BodySelectionError, GeneratedBodyRef, NativeSelections, SelectionMembers,
};
use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

impl BodySelection {
    /// Admit decoded local operands with a charged uniqueness index.
    pub fn local(
        bodies: Vec<String>,
        native: String,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, BodySelectionError>, cadmpeg_core::CodecError> {
        let bodies =
            NativeSelections::new(bodies, ctx, "validate distinct decoded native selections")?;
        ctx.charge_work_limit(
            cadmpeg_core::decode::u64_from_index(native.len()),
            "validate local selection reference",
        )?;
        Ok(bodies.and_then(|bodies| {
            Ok(Self::Local {
                bodies,
                native: native.try_into()?,
            })
        }))
    }

    /// Admit selection members with the decode context.
    pub fn historical(
        state: FeatureInputTopologyId,
        bodies: Vec<HistoricalBodyId>,
        native: String,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, BodySelectionError>, cadmpeg_core::CodecError> {
        let bodies = match SelectionMembers::new(
            bodies,
            ctx,
            "validate BodySelection historical members",
        )? {
            Ok(members) => members,
            Err(error) => return Ok(Err(error)),
        };
        ctx.charge_work_limit(
            cadmpeg_core::decode::u64_from_index(native.len()),
            "validate selection native reference",
        )?;
        let native = match native.try_into() {
            Ok(native) => native,
            Err(error) => return Ok(Err(error)),
        };
        Ok(Ok(Self::Historical {
            state,
            bodies,
            native,
        }))
    }

    /// Admit selection members with the decode context.
    pub fn generated(
        bodies: Vec<GeneratedBodyRef>,
        native: String,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Result<Self, BodySelectionError>, cadmpeg_core::CodecError> {
        let bodies =
            match SelectionMembers::new(bodies, ctx, "validate BodySelection generated members")? {
                Ok(members) => members,
                Err(error) => return Ok(Err(error)),
            };
        ctx.charge_work_limit(
            cadmpeg_core::decode::u64_from_index(native.len()),
            "validate selection native reference",
        )?;
        let native = match native.try_into() {
            Ok(native) => native,
            Err(error) => return Ok(Err(error)),
        };
        Ok(Ok(Self::Generated { bodies, native }))
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for BodySelectionError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(limit)
    }
}

impl From<BodySelectionError> for cadmpeg_core::CodecError {
    fn from(error: BodySelectionError) -> Self {
        match error {
            BodySelectionError::Resource(limit) => Self::ResourceLimit(limit),
            error => Self::malformed(error.to_string()),
        }
    }
}
