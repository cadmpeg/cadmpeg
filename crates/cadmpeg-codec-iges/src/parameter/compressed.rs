// SPDX-License-Identifier: Apache-2.0
//! Parameter admission from the owning Compressed Data callback.

use super::{FailedField, ParameterRecord};
use crate::directory::DirectoryEntry;
use crate::global::ResolvedGlobal;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Parse the Parameter Data of one Data record. A field whose literal is not
/// admitted has no token and is returned for its caller to give up.
pub(crate) fn parse(
    entry: &DirectoryEntry,
    lines: &[Vec<u8>],
    global: &ResolvedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(ParameterRecord, Vec<FailedField>)>, CodecError> {
    if entry.entity_type == 0 && lines.is_empty() {
        return Ok(None);
    }
    let start = u32::try_from(entry.parameter_start)
        .map_err(|_| CodecError::malformed("Compressed Parameter start exceeds u32"))?;
    let count = u32::try_from(lines.len())
        .map_err(|_| CodecError::malformed("Compressed Parameter count exceeds u32"))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| CodecError::malformed("Compressed Parameter range overflows"))?;
    let (admitted, failed) =
        super::admitted::lex_lines(entry.sequence, entry.entity_type, lines, global, ctx)?;
    Ok(Some((admitted.bind_range(start..end), failed)))
}
