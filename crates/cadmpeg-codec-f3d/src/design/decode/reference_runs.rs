// SPDX-License-Identifier: Apache-2.0
//! Admitted traversal of reference runs.

use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Every value of `run` in run order, after admitting one visit per value.
/// A run stores either unlocated values or located rows; the empty half
/// admits nothing.
pub(in crate::design::decode) fn admit_reference_values<'run, T, O>(
    ctx: &DecodeContext<'_>,
    run: &'run ReferenceRun<T, O>,
    operation: &'static str,
) -> Result<impl Iterator<Item = &'run T> + 'run, CodecError> {
    let unlocated = ctx.admit_iter(run.unlocated_values().unwrap_or(&[]), operation)?;
    let located = ctx.admit_iter(run.located_rows().unwrap_or(&[]), operation)?;
    Ok(unlocated.chain(located.map(|row| &row.value)))
}
