// SPDX-License-Identifier: Apache-2.0
//! Exact mirror scopes and mirror construction binding.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::marked_record_reference;
use super::shared_frames::{unique_match, UniqueMatch};
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::operands::{parse_face_operand, reference_at};
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::sketch::{
    cached_owned_record_offsets, indexed_record_header_at, next_indexed_record_header,
    IndexedRecordOffsets,
};
use crate::design::decode::text::relaxed_guid_end;
use crate::layout::design_mirror_scope_class369_tail as mirror_369;
use crate::layout::design_mirror_scope_class391_tail as mirror_391;
use crate::layout::design_mirror_scope_class413_tail as mirror_413;
use crate::layout::design_mirror_scope_class440_tail as mirror_440;
use crate::layout::design_mirror_scope_class441_count_owner as mirror_441_count;
use crate::layout::design_mirror_scope_class441_tail as mirror_441;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::mirror;
use crate::records::feature::mirror::DesignMirrorConstruction;
use crate::records::feature::mirror::DesignMirrorScopeTolerance;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use crate::records::recipes::ConstructionRecipe;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;
use std::collections::HashMap;

/// Record headers keyed by their stream scope and record index.
type MirrorHeaderIndex<'h> = HashMap<(&'h str, u32), &'h DesignRecordHeader>;

/// Parse the class-441 Mirror count owner carried outside the ordinary owner
/// arena.
///
/// The scope's fourth reference names a class-426 frame paired with class 267.
/// Its exact compact scalar envelope carries count two and has no decoded
/// Design-parameter backlink.
fn exact_legacy_mirror_scope_count(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<(u32, u64)>, CodecError> {
    if (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) != ("441", "267") {
        return Ok(None);
    }
    let Some(count_record_index) = reference_at(scope.reference_members(), 3) else {
        return Ok(None);
    };
    let &[start, paired] = records.offsets(count_record_index) else {
        return Ok(None);
    };
    if paired.checked_sub(start) != Some(mirror_441_count::LEN)
        || indexed_record_header_at(bytes, paired).map(|header| header.class_tag) != Some(b"267")
    {
        return Ok(None);
    }
    let Some(frame) = bytes.get(start..paired) else {
        return Ok(None);
    };
    let Some(owner) = crate::design::decode::parameters::parse_parameter_owner(ctx, frame)? else {
        return Ok(None);
    };
    let (Some(parameter_record_index), Some(companion_record_index), Ok(count_offset)) = (
        count_record_index.checked_add(2),
        count_record_index.checked_add(1),
        i128::try_from(mirror_441_count::COUNT),
    ) else {
        return Ok(None);
    };
    if owner.class_tag.as_str() != "426"
        || owner.record_index != count_record_index
        || owner.scope_record_index != scope.record_index
        || owner.local_ordinal != mirror_441_count::LOCAL_ORDINAL_VALUE
        || owner.owned_ordinal != mirror_441_count::OWNED_ORDINAL_VALUE
        || owner.evaluated_value.get() != f64::from(mirror_441_count::COUNT_VALUE)
        || owner.frame_length != u64_from_index(mirror_441_count::LEN)
        || owner.parameter_record_index != parameter_record_index
        || owner.companion_record_index != companion_record_index
        || owner.evaluated_value_offset
            != crate::design::decode::parameters::FrameRelative(count_offset)
    {
        return Ok(None);
    }
    Ok(owner
        .evaluated_value_offset
        .absolute(u64_from_index(start))
        .map(|offset| (count_record_index, offset)))
}

/// Parse a legacy Mirror scalar lane.
///
/// These forms have no ordinal-one parameter owner. Their positive stitch
/// tolerance is carried after the preceding-history field, with two marked
/// references naming the adjacent legacy records. The class pair selects the
/// generation-specific scalar marker; the remaining tail offsets are shared.
fn exact_legacy_mirror_scope_tolerance(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(PositiveReal, u64, DesignMirrorScopeTolerance)> {
    let (
        tail_length,
        previous_state,
        scalar_marker,
        stitch_tolerance,
        repeated_marker,
        first_reference,
        second_reference,
        marker_value,
    ) = match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
        ("369", "261") => (
            mirror_369::LEN,
            mirror_369::PREVIOUS_HISTORY_STATE,
            mirror_369::SCALAR_MARKER,
            mirror_369::STITCH_TOLERANCE,
            Some(mirror_369::REPEATED_SCALAR_MARKER),
            mirror_369::FIRST_REFERENCE,
            mirror_369::SECOND_REFERENCE,
            mirror_369::SCALAR_MARKER_VALUE,
        ),
        ("391", "261") => (
            mirror_391::LEN,
            mirror_391::PREVIOUS_HISTORY_STATE,
            mirror_391::SCALAR_MARKER,
            mirror_391::STITCH_TOLERANCE,
            Some(mirror_391::REPEATED_SCALAR_MARKER),
            mirror_391::FIRST_REFERENCE,
            mirror_391::SECOND_REFERENCE,
            mirror_391::SCALAR_MARKER_VALUE,
        ),
        ("413", "262") => (
            mirror_413::LEN,
            mirror_413::PREVIOUS_HISTORY_STATE,
            mirror_413::SCALAR_MARKER,
            mirror_413::STITCH_TOLERANCE,
            Some(mirror_413::REPEATED_SCALAR_MARKER),
            mirror_413::FIRST_REFERENCE,
            mirror_413::SECOND_REFERENCE,
            mirror_413::SCALAR_MARKER_VALUE,
        ),
        ("440", "258") => (
            mirror_440::LEN,
            mirror_440::PREVIOUS_HISTORY_STATE,
            mirror_440::SCALAR_MARKER,
            mirror_440::STITCH_TOLERANCE,
            Some(mirror_440::REPEATED_SCALAR_MARKER),
            mirror_440::FIRST_REFERENCE,
            mirror_440::SECOND_REFERENCE,
            mirror_440::SCALAR_MARKER_VALUE,
        ),
        ("441", "267") => (
            mirror_441::LEN,
            mirror_441::PREVIOUS_HISTORY_STATE,
            mirror_441::SCALAR_MARKER,
            mirror_441::STITCH_TOLERANCE,
            None,
            mirror_441::FIRST_REFERENCE,
            mirror_441::SECOND_REFERENCE,
            mirror_441::SCALAR_MARKER_VALUE,
        ),
        _ => return None,
    };
    // Both Mirror kinds have a fixed source name, so its UTF-16 length is a
    // constant of the kind.
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::Mirror(_) | scope::DesignScopePayload::SymetrieMiroir(_)
    ) {
        return None;
    }
    let kind_code_units = scope.kind_name().encode_utf16().count();
    let kind_end = usize::try_from(scope.kind_offset())
        .ok()?
        .checked_add(kind_code_units.checked_mul(2)?)?;
    let previous = usize::try_from(scope.previous_history_state_id_offset()?).ok()?;
    if previous != kind_end.checked_add(previous_state)? {
        return None;
    }
    let paired = usize::try_from(scope.paired_byte_offset()).ok()?;
    if paired != kind_end.checked_add(tail_length)?
        || paired.checked_sub(usize::try_from(scope.byte_offset()).ok()?)?
            != usize::try_from(scope.frame_length()).ok()?
    {
        return None;
    }
    let marker_offset = kind_end.checked_add(scalar_marker)?;
    let value_offset = kind_end.checked_add(stitch_tolerance)?;
    let repeated_marker_offset = repeated_marker.and_then(|offset| kind_end.checked_add(offset));
    let marker = View::u32_le_at(bytes, marker_offset)?;
    if marker != marker_value {
        return None;
    }
    if let Some(repeated_marker_offset) = repeated_marker_offset {
        if View::u32_le_at(bytes, repeated_marker_offset)? != marker_value {
            return None;
        }
    }
    let value = PositiveReal::new(View::f64_le_at(bytes, value_offset)?)?;
    let first_reference_offset = kind_end.checked_add(first_reference)?;
    let second_reference_offset = kind_end.checked_add(second_reference)?;
    let reference_slot = second_reference - first_reference;
    if bytes.get(first_reference_offset + 11..first_reference_offset + reference_slot)? != [0; 2]
        || bytes.get(second_reference_offset + 11..second_reference_offset + reference_slot)?
            != [0; 2]
        || second_reference_offset.checked_add(reference_slot)? != paired
    {
        return None;
    }
    let first_reference = marked_record_reference(bytes, first_reference_offset)?;
    let second_reference = marked_record_reference(bytes, second_reference_offset)?;
    if first_reference != scope.record_index.checked_add(2)?
        || second_reference != scope.record_index.checked_add(1)?
    {
        return None;
    }
    Some((
        value,
        u64_from_index(value_offset),
        DesignMirrorScopeTolerance {
            marker: mirror::DesignMirrorToleranceMarker::try_from((
                marker,
                repeated_marker_offset.map(u64_from_index),
            ))
            .ok()?,
            marker_offset: u64_from_index(marker_offset),
            first_reference,
            first_reference_offset: u64_from_index(first_reference_offset),
            second_reference,
            second_reference_offset: u64_from_index(second_reference_offset),
        },
    ))
}

/// Index record headers by stream scope and record index. A later header
/// replaces an earlier one with the same key.
fn mirror_record_headers<'h>(
    ctx: &DecodeContext<'_>,
    headers: &'h [DesignRecordHeader],
) -> Result<MirrorHeaderIndex<'h>, CodecError> {
    let mut index = HashMap::new();
    for header in ctx.admit_iter(headers, "scan F3D Mirror record headers")? {
        let Some(stream) = record_stream(ctx, &header.id)? else {
            continue;
        };
        ctx.insert_hash_map(
            &mut index,
            (stream, header.record_index),
            header,
            "f3d Mirror record headers",
        )?;
    }
    Ok(index)
}

/// The inputs a Mirror scope is joined with.
struct MirrorSources<'a, 'h> {
    scan: &'a ContainerScan<'a>,
    groups: &'a [DesignConstructionOperandGroup],
    headers: &'a MirrorHeaderIndex<'h>,
    owners: &'a [DesignParameterOwner],
    recipes: &'a [ConstructionRecipe],
}

/// Join a Mirror scope's two operand groups and fixed parameters with either a
/// referenced `WorkPlane` or a persistent plane-face selection.
///
/// The header index and the stream record indexes are built on the first
/// Mirror scope and held under one scoped reservation until binding ends.
pub(crate) fn bind_mirror_constructions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    owners: &[DesignParameterOwner],
    recipes: &[ConstructionRecipe],
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d Mirror binding indexes")?;
    let mut header_index = None;
    let mut stream_indexes = HashMap::new();
    for index in ctx.admit_iter(&(0..scopes.len()), "scan F3D Mirror scopes")? {
        if !matches!(
            scopes[index].payload(),
            scope::DesignScopePayload::Mirror(_) | scope::DesignScopePayload::SymetrieMiroir(_)
        ) {
            continue;
        }
        if header_index.is_none() {
            header_index = Some(storage.with_storage(|| mirror_record_headers(ctx, headers))?);
        }
        let Some(header_index) = &header_index else {
            continue;
        };
        let sources = MirrorSources {
            scan,
            groups,
            headers: header_index,
            owners,
            recipes,
        };
        let Some(construction) = mirror_construction(
            ctx,
            &sources,
            scopes,
            index,
            &mut storage,
            &mut stream_indexes,
        )?
        else {
            continue;
        };
        if let scope::DesignScopePayloadMut::Mirror(slot)
        | scope::DesignScopePayloadMut::SymetrieMiroir(slot) = scopes[index].payload_mut()
        {
            *slot = Some(construction);
        }
    }
    Ok(())
}

/// The record index of the stream holding `bytes`, built once per stream
/// under the binding's scoped reservation.
fn stream_record_offsets<'cache>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    stream_indexes: &'cache mut HashMap<String, IndexedRecordOffsets>,
    stream: &str,
    bytes: &[u8],
) -> Result<&'cache IndexedRecordOffsets, CodecError> {
    storage.with_storage(move || cached_owned_record_offsets(ctx, stream_indexes, stream, bytes))
}

/// The construction of the Mirror scope `scopes[index]`.
fn mirror_construction(
    ctx: &DecodeContext<'_>,
    sources: &MirrorSources<'_, '_>,
    scopes: &[DesignParameterScope],
    index: usize,
    storage: &mut ScopedReservation<'_>,
    stream_indexes: &mut HashMap<String, IndexedRecordOffsets>,
) -> Result<Option<DesignMirrorConstruction>, CodecError> {
    let Some(scope) = scopes.get(index) else {
        return Ok(None);
    };
    let Some(stream) = record_stream(ctx, &scope.id)? else {
        return Ok(None);
    };
    let Some(entry) = sources
        .scan
        .design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
    else {
        return Ok(None);
    };
    let bytes = sources.scan.entry_bytes(&entry.name)?;
    let scope_record_index = scope.record_index;
    let in_scope = |group: &DesignConstructionOperandGroup| -> Result<bool, CodecError> {
        Ok(group.scope_record_index == scope_record_index && in_stream(ctx, &group.id, stream)?)
    };
    let UniqueMatch::One(seed_group) = unique_match(
        ctx,
        sources.groups,
        |group| {
            Ok(matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            ) && in_scope(group)?)
        },
        "find F3D Mirror seed group",
    )?
    else {
        return Ok(None);
    };
    let UniqueMatch::One(plane_group) = unique_match(
        ctx,
        sources.groups,
        |group| Ok(group.role() == DesignOperandRole::ROLE_0X5 && in_scope(group)?),
        "find F3D Mirror plane group",
    )?
    else {
        return Ok(None);
    };
    let [crate::records::identity::Located {
        value: plane_member,
        ..
    }] = plane_group.members()
    else {
        return Ok(None);
    };
    let Some(&plane_header) = ctx.get_hash_map(
        sources.headers,
        &(stream, *plane_member),
        "find F3D Mirror record header",
    )?
    else {
        return Ok(None);
    };
    let work_plane = match compact_feature_reference(ctx, bytes, plane_header)? {
        Some((plane_reference, plane_reference_offset)) => match plane_reference.checked_add(1) {
            // A work-plane frame is present only on a `WorkPlane` scope.
            Some(record_index)
                if ctx.any_by(
                    scopes,
                    |candidate| {
                        Ok(candidate.record_index == record_index
                            && candidate.work_plane_frame().is_some()
                            && in_stream(ctx, &candidate.id, stream)?)
                    },
                    "find F3D Mirror work-plane scope",
                )? =>
            {
                Some((record_index, plane_reference_offset))
            }
            _ => None,
        },
        None => None,
    };
    let (plane_scope_record_index, plane_selection_record_index) =
        if let Some((plane_scope_record_index, plane_reference_offset)) = work_plane {
            (
                Some(crate::records::identity::Located {
                    value: plane_scope_record_index,
                    offset: plane_reference_offset,
                }),
                None,
            )
        } else {
            // Only whether an operand parses matters; the parsed operand is
            // held under scoped storage and dropped.
            let (entity_selection, _) =
                ctx.with_scoped_storage("f3d Mirror plane operand test", || {
                    Ok::<_, CodecError>(
                        crate::design::decode::operands::parse_entity_selection_operand(
                            ctx,
                            bytes,
                            plane_group,
                            0,
                            plane_header,
                        )?
                        .is_some(),
                    )
                })?;
            let selects_plane = entity_selection || {
                let records = stream_record_offsets(ctx, storage, stream_indexes, stream, bytes)?;
                ctx.with_scoped_storage("f3d Mirror plane operand test", || {
                    Ok::<_, CodecError>(
                        parse_face_operand(
                            ctx,
                            bytes,
                            records,
                            crate::design::decode::operands::FaceOperandFrame {
                                scope,
                                scope_reference_ordinal: plane_group.scope_reference_ordinal,
                                group_ownership: Some((plane_group.record_index, 0)),
                                next_byte_offset: None,
                                header: plane_header,
                            },
                            sources.recipes,
                        )?
                        .is_some(),
                    )
                })?
                .0
            };
            if !selects_plane {
                return Ok(None);
            }
            (None, Some(*plane_member))
        };
    let seed_feature = match seed_group.members() {
        _ if seed_group.role() != DesignOperandRole::BODIES_B => None,
        [crate::records::identity::Located { value: member, .. }] => {
            match ctx.get_hash_map(
                sources.headers,
                &(stream, *member),
                "find F3D Mirror record header",
            )? {
                Some(&header) => match compact_feature_reference(ctx, bytes, header)? {
                    Some((record_index, offset))
                        if ctx.any_by(
                            scopes,
                            |candidate| {
                                Ok(candidate.record_index == record_index
                                    && in_stream(ctx, &candidate.id, stream)?)
                            },
                            "find F3D Mirror seed-feature scope",
                        )? =>
                    {
                        Some((record_index, offset))
                    }
                    _ => None,
                },
                None => None,
            }
        }
        _ => None,
    };
    let in_scope_owner = |owner: &DesignParameterOwner| -> Result<bool, CodecError> {
        Ok(owner.scope_record_index() == scope_record_index && in_stream(ctx, owner.id(), stream)?)
    };
    let count = unique_match(
        ctx,
        sources.owners,
        |owner| {
            Ok(owner.local_ordinal() == 0
                && owner.evaluated_value().get() == 2.0
                && in_scope_owner(owner)?)
        },
        "find F3D Mirror count owner",
    )?;
    let records = stream_record_offsets(ctx, storage, stream_indexes, stream, bytes)?;
    let inline_count = exact_legacy_mirror_scope_count(ctx, bytes, records, scope)?;
    let count = match (count, inline_count) {
        (UniqueMatch::One(count), None) => (count.record_index(), count.evaluated_value_offset()),
        (UniqueMatch::Zero, Some(count)) => count,
        _ => return Ok(None),
    };
    let tolerance = unique_match(
        ctx,
        sources.owners,
        |owner| {
            Ok(owner.local_ordinal() == 1
                && PositiveReal::try_from(owner.evaluated_value()).is_ok()
                && in_scope_owner(owner)?)
        },
        "find F3D Mirror tolerance owner",
    )?;
    let (stitch_tolerance, stitch_tolerance_offset, tolerance_source) =
        match (tolerance, exact_legacy_mirror_scope_tolerance(bytes, scope)) {
            (UniqueMatch::One(owner), None) => {
                let Ok(value) = PositiveReal::try_from(owner.evaluated_value()) else {
                    return Ok(None);
                };
                (
                    value,
                    owner.evaluated_value_offset(),
                    mirror::DesignMirrorToleranceSource::Owner {
                        record_index: owner.record_index(),
                    },
                )
            }
            (UniqueMatch::Zero, Some((value, value_offset, scope_tail))) => (
                value,
                value_offset,
                mirror::DesignMirrorToleranceSource::Scope(scope_tail),
            ),
            _ => return Ok(None),
        };
    let (count_record_index, count_offset) = count;
    Ok(Some(DesignMirrorConstruction {
        count_record_index,
        count_offset,
        stitch_tolerance,
        stitch_tolerance_offset,
        tolerance_source,
        seed_group_record_index: seed_group.record_index,
        plane_group_record_index: plane_group.record_index,
        seed_feature_scope_record_index: seed_feature
            .map(|(value, offset)| crate::records::identity::Located { value, offset }),
        plane_scope_record_index,
        plane_selection_record_index,
        plane: None,
    }))
}

/// The feature record a compact persistent reference names: its two GUIDs,
/// then a paired header and three nested records whose indexes follow the
/// header's, the last carrying the referenced record index.
fn compact_feature_reference(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Result<Option<(u32, u64)>, CodecError> {
    let Ok(start) = usize::try_from(header.byte_offset) else {
        return Ok(None);
    };
    if !zeros_at::<10>(bytes, start + 11)
        || bytes.get(start + 21) != Some(&1)
        || View::u32_le_at(bytes, start + 22) != header.record_index.checked_add(3)
        || !zeros_at::<6>(bytes, start + 26)
        || View::u32_le_at(bytes, start + 32) != Some(1)
    {
        return Ok(None);
    }
    let Some(after_asset_id) = relaxed_guid_end(bytes, start + 36) else {
        return Ok(None);
    };
    let Some(after_context_id) = relaxed_guid_end(bytes, after_asset_id) else {
        return Ok(None);
    };
    if View::u32_le_at(bytes, after_context_id) != Some(2)
        || !zeros_at::<4>(bytes, after_context_id + 4)
    {
        return Ok(None);
    }
    // The paired header and the nested records carry consecutive indexes.
    let mut position = after_context_id + 8;
    let mut identity_at = 0;
    for delta in [0, 1, 2, 3] {
        let Some(next) = next_indexed_record_header(ctx, bytes, position, |_| true)? else {
            return Ok(None);
        };
        if Some(next.record_index) != header.record_index.checked_add(delta) {
            return Ok(None);
        }
        identity_at = next.offset;
        position = next.offset + 11;
    }
    // The zero runs around the reference leave no room for another header
    // before the one that must follow the identity record.
    if !zeros_at::<10>(bytes, identity_at + 11)
        || !zeros_at::<4>(bytes, identity_at + 25)
        || indexed_record_header_at(bytes, identity_at + 29).map(|next| next.record_index)
            != header.record_index.checked_add(4)
    {
        return Ok(None);
    }
    Ok(View::u32_le_at(bytes, identity_at + 21)
        .map(|reference| (reference, u64_from_index(identity_at + 21))))
}

#[cfg(test)]
mod tests;
