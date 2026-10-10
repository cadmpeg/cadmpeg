// SPDX-License-Identifier: Apache-2.0
//! Exact point-data levels and work-point constructions.

use super::parameter_scope::payload_prologue;
use super::shared_frames::find_frame;
use crate::bytes::finite_reals_at;
use crate::bytes::take_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::DesignWorkPointConstruction;
use crate::records::feature::work_geometry::DesignWorkPointInput;
use crate::records::feature::work_geometry::DesignWorkPointRule;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Type GUID of the point-data class a `WorkPoint` scope references. Every
/// record of this class carries the `point3d` member sequence below.
const POINT_DATA_TYPE_GUID: &str = "69EE2FA7-BCC7-449E-9CA9-976CEFDFED44";

/// Distance from an indexed-record header to the counted record name that
/// closes it: the eleven-byte header and one four-byte word.
const RECORD_NAME_OFFSET: usize = 15;

/// The end of the record name that closes the indexed-record header at
/// `header_at`: at most 256 graphic ASCII bytes. The byte test stops at the
/// first other byte and charges the bytes it reads under `operation`.
pub(super) fn record_name_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header_at: usize,
    operation: &'static str,
) -> Result<Option<usize>, CodecError> {
    let Some(count_at) = header_at.checked_add(RECORD_NAME_OFFSET) else {
        return Ok(None);
    };
    let Some(length) =
        View::u32_le_at(bytes, count_at).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if length > 256 {
        return Ok(None);
    }
    let start = count_at + 4;
    let end = start + length;
    let Some(name) = bytes.get(start..end) else {
        return Ok(None);
    };
    Ok(ctx
        .all_by(name, |byte| Ok(byte.is_ascii_graphic()), operation)?
        .then_some(end))
}

/// The base class level of a point-data record, read under one record version.
///
/// Two levels are equal exactly when they name the same members: the input
/// run's offset and length fix every reference in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointDataLevel {
    /// Byte offset of `point3d`'s first coordinate.
    position_at: usize,
    /// Construction rule that produced the point.
    reference_type: u32,
    /// Byte offset of the serialized construction rule.
    reference_type_at: usize,
    /// Byte offset of the first marked reference of the counted input run.
    inputs_at: usize,
    /// Serialized length of the input run.
    input_count: usize,
}

/// Read the base class level of the point-data class at `start` under one
/// record version.
///
/// The level closes with a counted reference run. The serialized count is the
/// only framing authority for that run: `reference_type` selects the
/// construction rule, but its rule-specific arity is not encoded in the class
/// member order. The count is bounded by the frame, and each marked reference
/// must resolve to a record index.
fn point_data_level(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    version: u32,
) -> Result<Option<PointDataLevel>, CodecError> {
    let Some(body) = bytes.get(..end) else {
        return Ok(None);
    };
    let Some(cursor) = payload_prologue(bytes, start, end) else {
        return Ok(None);
    };
    let Some(level) = (|| {
        let mut cursor = cursor;
        if version >= 2 {
            cursor = cursor.checked_add(4)?;
        }
        cursor = cursor.checked_add(16)?;
        if version >= 1 {
            take_reference(body, &mut cursor)?;
        }
        let position_at = cursor;
        let reference_type_at = position_at.checked_add(24)?;
        let reference_type = View::u32_le_at(body, reference_type_at)?;
        cursor = reference_type_at.checked_add(4)?;
        if version >= 3 {
            cursor = cursor.checked_add(24)?;
        }
        let input_count = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
        let inputs_at = cursor.checked_add(4)?;
        if input_count == 0 || input_count > end.checked_sub(inputs_at)? {
            return None;
        }
        Some(PointDataLevel {
            position_at,
            reference_type,
            reference_type_at,
            inputs_at,
            input_count,
        })
    })() else {
        return Ok(None);
    };
    // The count is read from the frame, so each reference is admitted as the
    // scan reaches it; the scan stops at the first unmarked one.
    let mut cursor = level.inputs_at;
    for _ in 0..level.input_count {
        ctx.charge_work(1, "scan F3D point-data inputs")?;
        if input_reference(body, &mut cursor).is_none() {
            return Ok(None);
        }
    }
    Ok(Some(level))
}

/// The marked input reference at `cursor`, advancing past it.
fn input_reference(body: &[u8], cursor: &mut usize) -> Option<DesignWorkPointInput> {
    let reference_offset = cursor.checked_add(1)?;
    let reference = take_reference(body, cursor)?;
    DesignWorkPointInput::try_new(
        u32::try_from(reference.target()?).ok()?,
        u64_from_index(reference_offset),
        None,
    )
    .ok()
}

/// The one level that every version allowed for the record names. A stored
/// version allows only itself; an unregistered record allows every version.
fn unique_point_data_level(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    payload_at: usize,
    paired: usize,
    stored_version: Option<u32>,
) -> Result<Option<PointDataLevel>, CodecError> {
    let versions = match stored_version {
        Some(version) => [Some(version), None, None, None],
        None => [Some(0), Some(1), Some(2), Some(3)],
    };
    let mut unique = None;
    for version in versions.into_iter().flatten() {
        let Some(level) = point_data_level(ctx, bytes, payload_at, paired, version)? else {
            continue;
        };
        if unique.is_some_and(|previous| previous != level) {
            return Ok(None);
        }
        unique = Some(level);
    }
    Ok(unique)
}

/// The coordinate of a `WorkPoint`'s point-data record.
///
/// The versions differ by members before and after `point3d`, so a version that
/// moves `point3d` names a different coordinate. `stream_types` carries the
/// record's own type GUID and version from its segment's type table: the GUID
/// identifies the point-data class and the version settles the frame outright.
/// Where the record's entity is not registered there, the class cannot be named
/// and every version whose member sequence fits the frame stays a candidate, so
/// the frame is read only when they agree on the offset.
pub(super) fn exact_work_point_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignWorkPointConstruction>, CodecError> {
    if !matches!(scope.payload(), scope::DesignScopePayload::WorkPoint(_)) {
        return Ok(None);
    }
    exact_point_data_construction(
        ctx,
        bytes,
        records,
        scope.reference_members().values().copied(),
        stream_types,
    )
}

/// The only point-data frame among the frames of `point_record_indices`
/// whose level reads a finite position. Each record index is admitted as the
/// search reaches it, and the search stops at a second frame. The input run
/// is copied into the output only after the frame is known to be the only one.
pub(in crate::design::decode) fn exact_point_data_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    point_record_indices: impl Iterator<Item = u32>,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignWorkPointConstruction>, CodecError> {
    let mut candidate = None;
    for record_index in point_record_indices {
        ctx.charge_work(1, "scan F3D point-data record indexes")?;
        // A class tag is `256` plus an index into the segment's own type
        // table, so it names a different class in every segment and cannot
        // select the point-data class. The type GUID can.
        let stored = stream_types.get(&u64::from(record_index)).copied();
        if let Some((type_guid, _)) = stored {
            // A fixed 36-byte comparison.
            if type_guid.as_bytes() != POINT_DATA_TYPE_GUID.as_bytes() {
                continue;
            }
        }
        let second = find_frame(
            ctx,
            records,
            record_index,
            |start, paired| {
                let Some(payload_at) =
                    record_name_end(ctx, bytes, start, "validate F3D point-data ASCII field")?
                else {
                    return Ok(false);
                };
                let Some(level) = unique_point_data_level(
                    ctx,
                    bytes,
                    payload_at,
                    paired,
                    stored.map(|(_, version)| version),
                )?
                else {
                    return Ok(false);
                };
                if finite_reals_at::<3>(bytes, level.position_at).is_none() {
                    return Ok(false);
                }
                Ok(candidate
                    .replace((record_index, start, paired, level))
                    .is_some())
            },
            "scan F3D point-data frames",
        )?;
        if second.is_some() {
            return Ok(None);
        }
    }
    let Some((record_index, start, paired, level)) = candidate else {
        return Ok(None);
    };
    point_data_construction(ctx, bytes, record_index, start, paired, level)
}

/// The construction of a validated point-data level, with its input run
/// copied into retained storage.
fn point_data_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    record_index: u32,
    start: usize,
    paired: usize,
    level: PointDataLevel,
) -> Result<Option<DesignWorkPointConstruction>, CodecError> {
    let (Some(body), Some(position)) = (
        bytes.get(..paired),
        finite_reals_at(bytes, level.position_at),
    ) else {
        return Ok(None);
    };
    // `point_data_level` read every input, so the copy runs to the end.
    let mut inputs = ctx.vector_storage(level.input_count, "f3d point-data inputs")?;
    let mut cursor = level.inputs_at;
    for _ in ctx.admit_iter(&(0..level.input_count), "scan F3D point-data inputs")? {
        let Some(input) = input_reference(body, &mut cursor) else {
            return Ok(None);
        };
        ctx.push_vec(&mut inputs, input, "f3d point-data inputs")?;
    }
    let Ok(rule) = DesignWorkPointRule::from_serialized(level.reference_type, inputs) else {
        return Ok(None);
    };
    Ok(Some(DesignWorkPointConstruction {
        point_record_index: record_index,
        point_record_byte_offset: u64_from_index(start),
        position,
        position_offset: u64_from_index(level.position_at),
        rule,
        reference_type_offset: u64_from_index(level.reference_type_at),
    }))
}

#[cfg(test)]
mod tests;
