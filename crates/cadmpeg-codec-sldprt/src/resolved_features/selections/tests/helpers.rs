// SPDX-License-Identifier: Apache-2.0
//! Exact-byte test adapters for native selection parser assertions.

use super::super::{
    compact_component_reference_list, compact_component_reference_list_at,
    compact_sketch_surface_component_path_at, compact_surface_selection_at,
    compact_termination_reference_path_at, component_vector_path_at,
    counted_surface_component_path_at, flatten_surface_references, inline_surface_reference_at,
    read_compact_body_ids, COMPACT_EDGE_VECTOR_MARKER,
};
use crate::records::FeatureInputComponentPathEntry;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn surface_reference_matches_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
    expected: &[FeatureInputComponentPathEntry],
) -> Result<bool, CodecError> {
    let candidates = [
        compact_surface_selection_at(payload, marker),
        component_vector_path_at(payload, marker),
        flatten_surface_references(compact_component_reference_list_at(payload, marker)),
        flatten_surface_references(compact_component_reference_list(payload, marker, false)),
        counted_surface_component_path_at(payload, marker),
        compact_termination_reference_path_at(ctx, payload, marker)?,
        compact_sketch_surface_component_path_at(payload, marker),
        inline_surface_reference_at(payload, marker),
    ];
    for components in candidates.into_iter().flatten() {
        if components == expected {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn compact_body_selection_at(payload: &[u8], offset: usize) -> Option<Vec<u32>> {
    let header_end = offset.checked_add(12)?;
    let schema_end = offset.checked_add(4)?;
    if payload.get(offset..schema_end) != Some(11000u32.to_le_bytes().as_slice())
        || payload.get(schema_end..header_end) != Some(&[0; 8])
    {
        if payload.get(offset..schema_end).is_none()
            || payload.get(schema_end..header_end).is_none()
        {
            return None;
        }
        return move_body_selection_at(payload, offset);
    }
    let ids = (|| {
        let count = usize::try_from(View::u32_le_at(payload, offset.checked_add(12)?)?).ok()?;
        let ids_start = offset.checked_add(16)?;
        let ids_end = ids_start.checked_add(count.checked_mul(4)?)?;
        let sentinel_end = ids_end.checked_add(4)?;
        let zeros_end = sentinel_end.checked_add(12)?;
        if payload.get(ids_end..sentinel_end)? != u32::MAX.to_le_bytes()
            || payload.get(sentinel_end..zeros_end)? != [0; 12]
        {
            return None;
        }
        payload.get(ids_start..ids_end)
    })();
    ids.map(read_compact_body_ids)
}

pub(in crate::resolved_features) fn selection_vector_tail(
    payload: &mut Vec<u8>,
    entries: &[u32],
) -> usize {
    let count = u32::try_from(entries.len()).expect("selection vector entry count fits u32");
    payload.extend_from_slice(&count.to_le_bytes());
    payload.extend_from_slice(&[0, 2, 0, 0]);
    payload.extend_from_slice(&[0, 0, 0, 0]);
    let marker = payload.len();
    payload.extend_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    payload.extend_from_slice(&[0, 0]);
    for local_id in entries {
        payload.extend_from_slice(&[0x32, 0x80, 0, 0]);
        payload.extend_from_slice(&[1; 12]);
        payload.extend_from_slice(&local_id.to_le_bytes());
    }
    marker
}

fn move_body_selection_at(payload: &[u8], offset: usize) -> Option<Vec<u32>> {
    crate::resolved_features::direct_edits::move_body_translation_record(
        payload,
        offset,
        payload.len(),
        u64_from_index(offset),
    )
    .filter(|record| record.selection_offset == offset)
    .map(|record| record.local_body_ids)
}
