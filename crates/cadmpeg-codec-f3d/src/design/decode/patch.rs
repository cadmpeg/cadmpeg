// SPDX-License-Identifier: Apache-2.0
//! Decode the boundary-settings record a `SurfacePatch` scope references once
//! per boundary component.

use super::sketch::IndexedRecordOffsets;
use crate::records::feature::surface_ops::{
    DesignPatchContinuity, DesignSurfacePatchBoundary, DesignSurfacePatchSelection,
};
use cadmpeg_core::decode::View;

/// The boundary-settings records a `SurfacePatch` scope references, in scope
/// reference order.
///
/// The settings record occupies one fixed ordinal per boundary component in
/// each settings-bearing `SurfacePatch` scope form. Every reference member is
/// offered to the record grammar and only the members it closes are kept. The
/// single-group path form carries no settings record and therefore yields none.
pub(super) fn surface_patch_boundaries(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    reference_members: &[u32],
) -> Vec<DesignSurfacePatchBoundary> {
    let mut boundaries = Vec::new();
    for (ordinal, record_index) in reference_members.iter().enumerate() {
        let Some(mut boundary) = records.first_at_or_after(0, *record_index).and_then(|at| {
            let end = records
                .frames(*record_index)
                .find_map(|(start, end)| (start == at).then_some(end))?;
            exact_surface_patch_boundary(bytes.get(..end)?, at)
        }) else {
            continue;
        };
        let Ok(ordinal) = u32::try_from(ordinal) else {
            continue;
        };
        boundary.scope_reference_ordinal = ordinal;
        boundary.record_index = *record_index;
        boundaries.push(boundary);
    }
    boundaries
}

/// One boundary-settings record read at the indexed header offset `at`.
///
/// The class level is two zero bytes, `u8 IsSeedSel`, `u32 PatchContinuity`,
/// `u32 PatchFlip`, `f64 PatchScale`, and the `rPatchModelRef` reference. The
/// base level's reference run closes the record and carries no settings.
fn exact_surface_patch_boundary(bytes: &[u8], at: usize) -> Option<DesignSurfacePatchBoundary> {
    use crate::layout::surface_patch_boundary_settings_prefix as layout;
    let frame = bytes.get(at..)?;
    let name_offset = layout::PROLOGUE.checked_sub(4)?;
    if View::u32_le_at(frame, name_offset)? != 0
        || frame.get(layout::PROLOGUE..layout::IS_SEED_SELECTION)? != [0; 2]
    {
        return None;
    }
    let is_seed_selection = match frame.get(layout::IS_SEED_SELECTION)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let scale = cadmpeg_ir::scalar::FiniteReal::new(View::f64_le_at(frame, layout::RAW_WEIGHT)?)?;
    let mut cursor = layout::LEN;
    let model_reference =
        u32::try_from(crate::bytes::take_reference(frame, &mut cursor)?.local()?.0).ok()?;
    Some(DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 0,
        is_seed_selection,
        continuity: DesignPatchContinuity::from_code(View::u32_le_at(frame, layout::CONTINUITY)?),
        flip: View::u32_le_at(frame, layout::SIDE_ORDINAL)?,
        scale,
        model_reference,
    })
}

const SURFACE_PATCH_TYPE: &str = "BF77AE1A-E8F6-43AB-A325-A2C984AF1C6B";

pub(super) fn surface_patch_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    scope: &crate::records::feature::scope::DesignParameterScope,
    type_info: Option<(&str, u32)>,
) -> Result<Option<DesignSurfacePatchSelection>, cadmpeg_core::CodecError> {
    use crate::layout::surface_patch_selection_prefix as prefix;
    use crate::layout::surface_patch_v5_interior_list_prefix as v5_interior;
    use crate::layout::surface_patch_v5_selection_frame as v5_frame;
    use crate::layout::surface_patch_v5_tool_reference_prefix as v5_tool;
    use crate::layout::surface_patch_v6_interior_list_prefix as v6_interior;
    use crate::layout::surface_patch_v6_tool_reference_prefix as v6_tool;
    use crate::layout::surface_patch_v7_normal_flip_prefix as v7_frame;
    use crate::records::feature::scope::DesignFeatureKind;

    macro_rules! parsed {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    if scope.kind() != DesignFeatureKind::SurfacePatch {
        return Ok(None);
    }
    let (guid, version) = parsed!(type_info);
    if !guid.eq_ignore_ascii_case(SURFACE_PATCH_TYPE) {
        return Ok(None);
    }
    let (start, interior, tool) = match version {
        5 => (
            v5_frame::SELECTION_PREFIX,
            (
                v5_interior::ZERO_RUN,
                v5_interior::INTERIOR_COUNT,
                v5_interior::LEN,
            ),
            (v5_tool::ZERO_RUN, v5_tool::LEN),
        ),
        6 => (
            v5_frame::SELECTION_PREFIX,
            (
                v6_interior::ZERO_RUN,
                v6_interior::INTERIOR_COUNT,
                v6_interior::LEN,
            ),
            (v6_tool::ZERO_RUN, v6_tool::LEN),
        ),
        7 => (
            v7_frame::FOLLOWING_WORD,
            (
                v6_interior::ZERO_RUN,
                v6_interior::INTERIOR_COUNT,
                v6_interior::LEN,
            ),
            (v6_tool::ZERO_RUN, v6_tool::LEN),
        ),
        _ => return Ok(None),
    };
    let start_offset = parsed!(usize::try_from(scope.byte_offset()).ok());
    let end_offset = parsed!(usize::try_from(scope.paired_byte_offset()).ok());
    let frame = parsed!(bytes.get(start_offset..end_offset));
    if parsed!(View::u32_le_at(
        frame,
        parsed!(start.checked_add(prefix::LEADING_WORD))
    )) != prefix::LEADING_WORD_VALUE
        || parsed!(View::u32_le_at(
            frame,
            parsed!(start.checked_add(prefix::SELECTION_WORD))
        )) != prefix::SELECTION_WORD_VALUE
    {
        return Ok(None);
    }
    let settings_count = parsed!(View::u32_le_at(
        frame,
        parsed!(start.checked_add(prefix::SETTINGS_COUNT))
    ));
    let mut cursor = parsed!(start.checked_add(prefix::LEN));
    let settings = parsed!(local_references(ctx, frame, &mut cursor, settings_count)?);
    let (zero_run, count_at, interior_len) = interior;
    if !zero_bytes(
        frame,
        parsed!(cursor.checked_add(zero_run)),
        parsed!(cursor.checked_add(count_at)),
    ) {
        return Ok(None);
    }
    let interior_count = parsed!(View::u32_le_at(
        frame,
        parsed!(cursor.checked_add(count_at))
    ));
    cursor = parsed!(cursor.checked_add(interior_len));
    let interior_groups = parsed!(local_references(ctx, frame, &mut cursor, interior_count)?);
    let (zero_run, tool_len) = tool;
    if !zero_bytes(
        frame,
        parsed!(cursor.checked_add(zero_run)),
        parsed!(cursor.checked_add(tool_len)),
    ) {
        return Ok(None);
    }
    cursor = parsed!(cursor.checked_add(tool_len));
    let tool_body = parsed!(local_reference(frame, &mut cursor));
    Ok(Some(DesignSurfacePatchSelection {
        settings,
        interior_groups,
        tool_body,
    }))
}

fn zero_bytes(frame: &[u8], start: usize, end: usize) -> bool {
    frame
        .get(start..end)
        .is_some_and(|run| run.iter().all(|byte| *byte == 0))
}

fn local_reference(frame: &[u8], cursor: &mut usize) -> Option<u32> {
    u32::try_from(crate::bytes::take_reference(frame, cursor)?.local()?.0).ok()
}

fn local_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &[u8],
    cursor: &mut usize,
    count: u32,
) -> Result<Option<Vec<u32>>, cadmpeg_core::CodecError> {
    let Some(remaining) = frame.len().checked_sub(*cursor) else {
        return Ok(None);
    };
    let Some(count) = cadmpeg_core::decode::bounded_len(u64::from(count), 11, remaining) else {
        return Ok(None);
    };
    let mut references = ctx.alloc_filled(count, 0, "f3d surface-patch references")?;
    for reference in &mut references {
        let Some(value) = local_reference(frame, cursor) else {
            return Ok(None);
        };
        *reference = value;
    }
    Ok(Some(references))
}

pub(super) fn surface_patch_normal(
    bytes: &[u8],
    scope: &crate::records::feature::scope::DesignParameterScope,
    type_info: Option<(&str, u32)>,
) -> Option<crate::records::feature::surface_ops::DesignSurfacePatchNormalState> {
    use crate::layout::surface_patch_v7_normal_flip_prefix as layout;
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::feature::surface_ops::DesignSurfacePatchNormalState;
    use crate::records::identity::Located;
    if scope.kind() != DesignFeatureKind::SurfacePatch {
        return None;
    }
    let Some((guid, version)) = type_info else {
        return Some(DesignSurfacePatchNormalState::Unsupported);
    };
    if !guid.eq_ignore_ascii_case(SURFACE_PATCH_TYPE) {
        return Some(DesignSurfacePatchNormalState::Unsupported);
    }
    match version {
        1 | 5 | 6 => return None,
        7 => {}
        _ => return Some(DesignSurfacePatchNormalState::Unsupported),
    }
    let admitted = || {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let end = usize::try_from(scope.paired_byte_offset()).ok()?;
        let frame = bytes.get(start..end)?;
        if View::u32_le_at(frame, layout::FOLLOWING_WORD) != Some(layout::FOLLOWING_WORD_VALUE) {
            return None;
        }
        let reversed = match *frame.get(layout::NORMAL_FLIP)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        Some(DesignSurfacePatchNormalState::Explicit {
            reversed: Located {
                value: reversed,
                offset: scope
                    .byte_offset()
                    .checked_add(u64::try_from(layout::NORMAL_FLIP).ok()?)?,
            },
        })
    };
    Some(admitted().unwrap_or(DesignSurfacePatchNormalState::Unsupported))
}

#[cfg(test)]
mod tests;
