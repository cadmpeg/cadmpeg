// SPDX-License-Identifier: Apache-2.0
//! Tokenized Parameter content before its actual renderer binds card positions.

use super::{
    lex_admitted, reject_failed_fields, AdmittedParameter, FailedField, OwnedParameterBytes,
    OwnedParseFailure,
};
use crate::global::ResolvedGlobal;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(crate) fn parse_lines(
    directory_sequence: u32,
    entity_type: i64,
    lines: &[Vec<u8>],
    global: &ResolvedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<AdmittedParameter, CodecError> {
    let (admitted, failed) = lex_lines(directory_sequence, entity_type, lines, global, ctx)?;
    reject_failed_fields(&failed).map_err(|error| failure(directory_sequence, error))?;
    Ok(admitted)
}

/// Lex rendered or source lines of 64 columns. A field whose literal is not
/// admitted has no token and is returned for its caller to give up.
pub(crate) fn lex_lines(
    directory_sequence: u32,
    entity_type: i64,
    lines: &[Vec<u8>],
    global: &ResolvedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<(AdmittedParameter, Vec<FailedField>), CodecError> {
    let byte_len = lines
        .len()
        .checked_mul(64)
        .ok_or_else(|| CodecError::malformed("Compressed Parameter bytes overflow"))?;
    ctx.retain_bytes(u64_from_index(byte_len), "IGES Parameter syntax bytes")?;
    let mut bytes = ctx.alloc_filled(byte_len, b' ', "IGES Parameter syntax bytes")?;
    let mut boundaries = ctx.alloc_filled(lines.len(), 0_usize, "IGES Parameter boundaries")?;
    for (line, target) in lines.iter().zip(bytes.chunks_exact_mut(64)) {
        let target = target
            .get_mut(..line.len())
            .ok_or_else(|| CodecError::malformed("Compressed Parameter line exceeds 64 columns"))?;
        target.copy_from_slice(line);
    }
    for (index, boundary) in boundaries.iter_mut().enumerate() {
        *boundary = index
            .checked_add(1)
            .and_then(|index| index.checked_mul(64))
            .ok_or_else(|| CodecError::malformed("Parameter boundary overflows"))?;
    }
    lex_admitted(
        directory_sequence,
        entity_type,
        OwnedParameterBytes {
            bytes,
            card_boundaries: boundaries,
        },
        global,
    )
    .map_err(|error| failure(directory_sequence, error))
}

fn failure(directory_sequence: u32, error: OwnedParseFailure) -> CodecError {
    match error {
        OwnedParseFailure::Error(error) => error,
        OwnedParseFailure::Defect(defect, _) => CodecError::malformed(format!(
            "Directory D{} Parameter Data: {}",
            directory_sequence,
            defect.describe()
        )),
    }
}
