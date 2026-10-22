// SPDX-License-Identifier: Apache-2.0
//! Parameter admission from the owning Compressed Data callback.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use crate::directory::DirectoryEntry;
use crate::global::ResolvedGlobal;

use super::{
    finish_records, parse_owned_record, OwnedParameterBytes, OwnedParseFailure, Ownership,
    ParameterAssembly, ParameterRecord,
};

pub(crate) fn parse(
    entry: &DirectoryEntry,
    lines: &[Vec<u8>],
    global: &ResolvedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<Option<ParameterRecord>, CodecError> {
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
    let byte_len = lines
        .len()
        .checked_mul(64)
        .ok_or_else(|| CodecError::malformed("Compressed Parameter bytes overflow"))?;
    ctx.retain_bytes(u64_from_index(byte_len), "IGES Parameter syntax bytes")?;
    let mut bytes = ctx.alloc_filled(byte_len, b' ', "IGES Parameter syntax bytes")?;
    let mut cards = ctx.alloc_filled(lines.len(), 0_u32, "IGES Compressed Parameter cards")?;
    let mut boundaries = ctx.alloc_filled(lines.len(), 0_usize, "IGES Parameter boundaries")?;
    for (line, target) in lines.iter().zip(bytes.chunks_exact_mut(64)) {
        let target = target
            .get_mut(..line.len())
            .ok_or_else(|| CodecError::malformed("Compressed Parameter line exceeds 64 columns"))?;
        target.copy_from_slice(line);
    }
    for ((sequence, value), boundary) in (start..end).zip(&mut cards).zip(&mut boundaries) {
        *value = sequence;
        *boundary = usize::try_from(sequence - start + 1)
            .ok()
            .and_then(|index| index.checked_mul(64))
            .ok_or_else(|| CodecError::malformed("Compressed Parameter boundary overflows"))?;
    }
    let owned = Ownership {
        entry,
        cards,
        quarantine: None,
    };
    parse_owned_record(
        &owned,
        OwnedParameterBytes {
            bytes,
            card_boundaries: boundaries,
        },
        global,
    )
    .map(Some)
    .map_err(|error| match error {
        OwnedParseFailure::Error(error) => error,
        OwnedParseFailure::Defect(defect, _) => CodecError::malformed(format!(
            "Directory D{} Parameter Data: {}",
            entry.sequence,
            defect.describe()
        )),
    })
}

pub(crate) fn finish(
    directory: &[DirectoryEntry],
    mut records: Vec<ParameterRecord>,
    global: &ResolvedGlobal,
) -> ParameterAssembly {
    let entries: BTreeMap<_, _> = directory
        .iter()
        .filter(|entry| !(entry.entity_type == 0 && entry.parameter_line_count == 0))
        .map(|entry| (entry.sequence, entry))
        .collect();
    let trailing_pointer_analysis = finish_records(&entries, &mut records, global);
    ParameterAssembly {
        records,
        trailing_pointer_analysis,
        quarantined: Vec::new(),
        recoveries: crate::card::FramingRecoveries::default(),
        given_up: BTreeSet::new(),
    }
}
