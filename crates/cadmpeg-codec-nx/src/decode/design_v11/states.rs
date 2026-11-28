// SPDX-License-Identifier: Apache-2.0
//! The stored Parasolid states of an NX part (§3.1 Stored states).
//!
//! Each mark of a Deltas stream stores a kernel state of the part. The
//! decoder composes no mark into a design evaluation, so each stored state
//! stays in its native record and is charged by name.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::NxLossCode;
use crate::native::parasolid::deltas_metadata::stored_states;

#[cfg(test)]
mod tests;

/// Charges each stored Parasolid state of `ir` by its mark.
pub(in crate::decode) fn charge(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("nx") else {
        return Ok(());
    };
    let states = stored_states(ctx, namespace)?;
    if states.is_empty() {
        return Ok(());
    }
    let named = states
        .iter()
        .map(|state| {
            let direction = if state.forward { "forward" } else { "backward" };
            format!("{} (mark {}, {direction})", state.id, state.identifier)
        })
        .collect::<Vec<_>>();
    losses.push(NxLossCode::StoredStateUnread.note(format!(
        "NX stores {} Parasolid state(s) as Deltas marks; the decoder composes no mark into a design evaluation, and each stays in its native record: {}",
        named.len(),
        named.join(", ")
    )));
    Ok(())
}
