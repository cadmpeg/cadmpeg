// SPDX-License-Identifier: Apache-2.0
//! Regenerate neutral design records; native envelopes cannot override edits.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::native::F3dNative;

mod configurations;
mod parameters;

pub(super) fn regenerate(
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    // Configurations first: binding and canonicalizing the neutral model
    // folds the active overrides into their parameters. Parameters author
    // from that canonical neutral, so the generator's re-projection of the
    // authored native agrees with the neutral model it checks.
    let source_parameters = target.model.parameters.clone();
    configurations::regenerate(target, native, &source_parameters, losses)?;
    let remapped = parameters::regenerate(target, native, losses)?;
    // The parameter rewrite above re-identifies every authored parameter
    // from its native record. Retained configuration selections still name
    // the pre-rewrite identities, so point them at the live parameters. A
    // key no authored parameter continues is left for the projection
    // validator to decline.
    for configuration in &mut target.model.configurations {
        for (old, new) in &remapped {
            if old == new {
                continue;
            }
            if let Some(expression) = configuration.parameter_overrides.remove(old) {
                configuration
                    .parameter_overrides
                    .entry(new.clone())
                    .or_insert(expression);
            }
            if let Some(value) = configuration.parameter_values.remove(old) {
                configuration
                    .parameter_values
                    .entry(new.clone())
                    .or_insert(value);
            }
        }
    }
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
