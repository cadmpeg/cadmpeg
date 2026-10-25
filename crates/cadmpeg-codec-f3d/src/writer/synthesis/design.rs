// SPDX-License-Identifier: Apache-2.0
//! Regenerate neutral design records; native envelopes cannot override edits.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;

mod configurations;
mod parameters;

pub(super) fn regenerate(
    ctx: &DecodeContext<'_>,
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let source_parameters = target.model.parameters.clone();
    parameters::regenerate(ctx, target, native, losses)?;
    configurations::regenerate(target, native, &source_parameters, losses)?;
    Ok(())
}
