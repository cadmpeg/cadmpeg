// SPDX-License-Identifier: Apache-2.0
//! Regenerate neutral design records; native envelopes cannot override edits.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;

mod configurations;
mod parameters;
pub(super) mod sketch;

pub(super) fn regenerate(
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let source_parameters = target.model.parameters.clone();
    parameters::regenerate(target, native, losses)?;
    configurations::regenerate(target, native, &source_parameters, losses)?;
    Ok(())
}

pub(super) fn with_context<T>(
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    run(&ctx)
}
