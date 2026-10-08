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

/// The position of the first value of `run` that `predicate` selects. Each
/// visited value is admitted before its test; the search stops at the match.
pub(in crate::design::decode) fn reference_position<T, O>(
    ctx: &DecodeContext<'_>,
    run: &ReferenceRun<T, O>,
    mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<usize>, CodecError> {
    match (run.unlocated_values(), run.located_rows()) {
        (Some(values), _) => ctx.position_by(values, predicate, operation),
        (None, Some(rows)) => ctx.position_by(rows, |row| predicate(&row.value), operation),
        (None, None) => Ok(None),
    }
}
