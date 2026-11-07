// SPDX-License-Identifier: Apache-2.0
//! Decode the boundary-settings record a `SurfacePatch` scope references once
//! per boundary component.

use crate::metastream::PrimaryFrames;
use crate::records::feature::surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary};
use cadmpeg_core::decode::View;

pub(super) fn contain_surface_patch_boundaries(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parts: &mut cadmpeg_ir::source_fidelity::parts::PartOutput,
    source: View<'_>,
    stream: &str,
    records: &[crate::metastream::PrimaryRecordFrame],
    scope: &crate::records::feature::scope::DesignParameterScope,
    types: &std::collections::HashMap<u64, (&str, u32)>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartSpec};
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfacePatch {
        return Ok(());
    }
    let refs: Vec<_> = scope.reference_members().values().copied().collect();
    let Some(count) = refs.len().checked_sub(1).filter(|n| *n > 0 && n % 3 == 0) else {
        return Ok(());
    };
    let Some(extra) = (count / 3 - 1).checked_mul(44) else {
        return Ok(());
    };
    let Some(length) = scope.frame_length().and_then(|n| usize::try_from(n).ok()) else {
        return Ok(());
    };
    if length != 340 + extra && length != 354 + extra {
        return Ok(());
    }
    let group = |index: u32| {
        types.get(&u64::from(index)).is_some_and(|(guid, _)| {
            guid.eq_ignore_ascii_case("2CA5A1CD-C99B-4C9B-91AF-57989148E841")
        })
    };
    let starts: Vec<_> = [0, 1]
        .into_iter()
        .filter(|start| (0..count / 3).all(|i| group(refs[start + i * 3])))
        .collect();
    let [start] = starts.as_slice() else {
        return Ok(());
    };
    for ordinal in (0..count / 3).map(|i| start + i * 3 + 2) {
        let index = refs[ordinal];
        for frame in records
            .iter()
            .filter(|frame| frame.entity_id == u64::from(index))
        {
            let begin = source.start().checked_add(frame.start).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("SurfacePatch boundary start overflows")
            })?;
            let end = source.start().checked_add(frame.end).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("SurfacePatch boundary end overflows")
            })?;
            let extent = source.child(begin, end).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("SurfacePatch boundary exceeds its stream")
            })?;
            let id = cadmpeg_ir::ids::Identity::new(super::text::design_record_id(
                stream,
                ":design-record-header#",
                cadmpeg_core::decode::u64_from_index(frame.start),
            ))
            .map_err(cadmpeg_core::CodecError::malformed)?;
            let mut spec = PartSpec::new("f3d.bulk_record")
                .holding([cadmpeg_ir::document::ArenaClass::Design])
                .record_id(id);
            if extent.location().space != cadmpeg_core::decode::SpaceId::ROOT {
                spec = spec.in_stream(
                    cadmpeg_ir::StreamName::try_from(stream.to_owned())
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                );
            }
            let stub_id = spec.unreadable_id(cadmpeg_ir::CodecFormat::F3d, extent)?;
            if parts.fidelity().retained_record(stub_id.as_str()).is_some() {
                continue;
            }
            ctx.decode_part(parts, extent, spec, |_| {
                exact_surface_patch_boundary(
                    source.window().get(..frame.end).unwrap_or_default(),
                    frame.start,
                )
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "SurfacePatch boundary entity {index} has an invalid frame"
                    ))
                })
            })?;
        }
    }
    Ok(())
}

/// The boundary-settings records a `SurfacePatch` scope references, in scope
/// reference order.
///
/// The settings record occupies one fixed ordinal per boundary component in
/// each settings-bearing `SurfacePatch` scope form. Every reference member is
/// offered to the record grammar and only the members it closes are kept. The
/// single-group path form carries no settings record and therefore yields none.
pub(super) fn surface_patch_boundaries(
    bytes: &[u8],
    records: &[crate::metastream::PrimaryRecordFrame],
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

pub(super) fn surface_patch_normal(
    bytes: &[u8],
    scope: &crate::records::feature::scope::DesignParameterScope,
    type_info: Option<(&str, u32)>,
) -> Option<crate::records::feature::surface_ops::DesignSurfacePatchNormalState> {
    use crate::layout::surface_patch_v7_normal_flip_prefix as layout;
    use crate::records::feature::scope::DesignFeatureKind;
    use crate::records::feature::surface_ops::DesignSurfacePatchNormalState;
    use crate::records::identity::Sourced;
    if scope.kind() != DesignFeatureKind::SurfacePatch {
        return None;
    }
    let Some((guid, version)) = type_info else {
        return Some(DesignSurfacePatchNormalState::Unsupported);
    };
    if !guid.eq_ignore_ascii_case("BF77AE1A-E8F6-43AB-A325-A2C984AF1C6B") {
        return Some(DesignSurfacePatchNormalState::Unsupported);
    }
    match version {
        1 | 5 | 6 => return None,
        7 => {}
        _ => return Some(DesignSurfacePatchNormalState::Unsupported),
    }
    let admitted = || {
        let start = usize::try_from(scope.byte_offset()?).ok()?;
        let end = usize::try_from(scope.paired_byte_offset()?).ok()?;
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
            reversed: Sourced {
                value: reversed,
                offset: Some(
                    scope
                        .byte_offset()?
                        .checked_add(u64::try_from(layout::NORMAL_FLIP).ok()?)?,
                ),
            },
        })
    };
    Some(admitted().unwrap_or(DesignSurfacePatchNormalState::Unsupported))
}

#[cfg(test)]
mod tests;
