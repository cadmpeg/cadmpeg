// SPDX-License-Identifier: Apache-2.0
//! Exact surface extend, offset, boundary, stitch and ruled-surface operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::marked_record_reference;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::operands::parse_construction_operand_group;
use crate::design::decode::operands::ConstructionOperandGroupParse;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::reference_runs::{admit_reference_values, reference_position};
use crate::design::decode::sketch::{indexed_record_header_at, IndexedRecordOffsets};
use crate::design::decode::text::{fixed_guid_ascii, retain_class_tag};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::feature::surface_ops::DesignRuledSurfaceCorner;
use crate::records::feature::surface_ops::DesignRuledSurfaceMethod;
use crate::records::feature::surface_ops::DesignRuledSurfaceOperation;
use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;
use crate::records::feature::surface_ops::DesignSurfaceStitchOperation;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

pub(super) fn exact_surface_extend_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceExtendOperation>, CodecError> {
    if !matches!(scope.payload(), DesignScopePayload::SurfaceExtend(_)) {
        return Ok(None);
    }
    let Some(operation) = exact_surface_boundary_operation(ctx, bytes, records, scope, 8)? else {
        return Ok(None);
    };
    if operation.distance.get() <= 0.0 {
        return Ok(None);
    }
    let method = match operation.mode {
        0 => DesignSurfaceExtendMethod::Natural,
        1 => DesignSurfaceExtendMethod::Tangent,
        2 => DesignSurfaceExtendMethod::Perpendicular,
        _ => return Ok(None),
    };
    Ok(Some(DesignSurfaceExtendOperation {
        distance: operation.distance,
        distance_offset: operation.distance_offset,
        distance_record_index: operation.distance_record_index,
        method,
        method_offset: operation.mode_offset,
        boundary_record_index: operation.boundary_record_index,
        boundary_reference_record_index: operation.boundary_reference_record_index,
        boundary_reference_offset: operation.boundary_reference_offset,
        edge_record_indices: boundary_edge_record_indices(ctx, scope)?,
        tolerance: operation.tolerance,
        tolerance_offset: operation.tolerance_offset,
    }))
}

pub(super) fn exact_surface_offset_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceOffsetOperation>, CodecError> {
    if !matches!(scope.payload(), DesignScopePayload::SurfaceOffset(_)) {
        return Ok(None);
    }
    if let Some(operation) = exact_surface_offset_face_groups(ctx, bytes, records, scope)? {
        return Ok(Some(operation));
    }
    let Some(operation) = exact_surface_boundary_operation(ctx, bytes, records, scope, 65)? else {
        return Ok(None);
    };
    if operation.mode != 1 {
        return Ok(None);
    }
    Ok(Some(DesignSurfaceOffsetOperation {
        distance: operation.distance,
        distance_offset: operation.distance_offset,
        distance_record_index: operation.distance_record_index,
        support: DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_record_index: operation.boundary_record_index,
            boundary_reference_record_index: operation.boundary_reference_record_index,
            boundary_reference_offset: operation.boundary_reference_offset,
            edge_record_indices: boundary_edge_record_indices(ctx, scope)?,
            tolerance: operation.tolerance,
            tolerance_offset: operation.tolerance_offset,
        },
    }))
}

/// A `SurfaceOffset` whose support references are profile face groups and
/// their members. Every support reference is either a group or a member of
/// exactly one group.
fn exact_surface_offset_face_groups(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceOffsetOperation>, CodecError> {
    let references = scope.reference_members();
    let Some(&distance_record_index) = references.values().next() else {
        return Ok(None);
    };
    let support_reference_count = references.len() - 1;
    if support_reference_count == 0 {
        return Ok(None);
    }
    let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, distance_record_index)? else {
        return Ok(None);
    };
    if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
        return Ok(None);
    }

    // A record is covered once as a group or a group member. It is tracked at
    // the first table position that lists it.
    let (mut covered, _covered_storage) =
        ctx.with_scoped_storage("track F3D surface offset covered references", || {
            ctx.alloc_filled(
                references.len(),
                false,
                "track F3D surface offset covered references",
            )
        })?;
    let mut covered_count = 0usize;
    let mut cover = |record_index: u32| -> Result<bool, CodecError> {
        let Some(position) = reference_position(
            ctx,
            references,
            |value| Ok(*value == record_index),
            "find F3D surface offset support reference",
        )?
        else {
            return Ok(false);
        };
        let Some(slot) = covered.get_mut(position) else {
            return Ok(false);
        };
        if std::mem::replace(slot, true) {
            return Ok(false);
        }
        covered_count += 1;
        Ok(true)
    };
    let mut group_record_indices = Vec::new();
    let mut scope_reference_ordinal = 0usize;
    let refused = reference_position(
        ctx,
        references,
        |&record_index| {
            let ordinal = scope_reference_ordinal;
            scope_reference_ordinal += 1;
            if ordinal == 0 {
                return Ok(false);
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(true);
            };
            let Some(group) = exact_construction_operand_group(
                ctx,
                bytes,
                records,
                scope,
                ordinal,
                record_index,
            )?
            else {
                return Ok(false);
            };
            if group.role() != DesignOperandRole::PROFILE
                || group.frame.opaque_index.get() != 252
                || group.members().is_empty()
                || !cover(group.record_index)?
            {
                return Ok(true);
            }
            let member_refused = ctx.any_by(
                group.members(),
                |member| {
                    // Every member is a support reference other than the distance owner.
                    Ok(member.value == distance_record_index || !cover(member.value)?)
                },
                "scan F3D surface offset group members",
            )?;
            if member_refused {
                return Ok(true);
            }
            ctx.push_vec(
                &mut group_record_indices,
                group.record_index,
                "f3d surface offset face group",
            )?;
            Ok(false)
        },
        "scan F3D surface offset support references",
    )?;
    if refused.is_some()
        || group_record_indices.is_empty()
        || covered_count != support_reference_count
    {
        return Ok(None);
    }
    Ok(Some(DesignSurfaceOffsetOperation {
        distance: scalar.value,
        distance_offset: scalar.value_offset,
        distance_record_index,
        support: DesignSurfaceOffsetSupport::FaceGroups {
            group_record_indices,
        },
    }))
}

/// The only construction operand group among the frames of `record_index`.
fn exact_construction_operand_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    record_index: u32,
) -> Result<Option<DesignConstructionOperandGroup>, CodecError> {
    let mut candidate = None;
    for (start, _) in records.frames(ctx, record_index)? {
        let Some(header) = indexed_record_header_at(bytes, start) else {
            return Ok(None);
        };
        // The group parser borrows the frame header; the group keeps its own copy.
        let (class_tag, _class_tag_storage) =
            ctx.with_scoped_storage("copy F3D surface offset group class tag", || {
                retain_class_tag(
                    ctx,
                    header.class_tag,
                    "copy F3D surface offset group class tag",
                )
            })?;
        let header = RecordFrame {
            record_index,
            class_tag,
            byte_offset: u64_from_index(start),
        };
        match parse_construction_operand_group(ctx, bytes, scope, scope_reference_ordinal, &header)?
        {
            ConstructionOperandGroupParse::Complete(group) => {
                if candidate.replace(*group).is_some() {
                    return Ok(None);
                }
            }
            ConstructionOperandGroupParse::NotAGroup | ConstructionOperandGroupParse::Unclosed => {}
        }
    }
    Ok(candidate)
}

/// The scalar fields of a boundary-carrier surface operation. Its edges are
/// the scope's references after the distance owner and the boundary record.
struct ExactSurfaceBoundaryOperation {
    distance: FiniteReal,
    distance_offset: u64,
    distance_record_index: u32,
    mode: u32,
    mode_offset: u64,
    boundary_record_index: u32,
    boundary_reference_record_index: u32,
    boundary_reference_offset: u64,
    tolerance: PositiveReal,
    tolerance_offset: u64,
}

fn exact_surface_boundary_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    boundary_kind: u32,
) -> Result<Option<ExactSurfaceBoundaryOperation>, CodecError> {
    let references = scope.reference_members();
    let mut values = references.values();
    let (Some(&distance_record_index), Some(&boundary_record_index)) =
        (values.next(), values.next())
    else {
        return Ok(None);
    };
    let edge_count = values.len();
    if edge_count == 0 {
        return Ok(None);
    }
    let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, distance_record_index)? else {
        return Ok(None);
    };
    if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
        return Ok(None);
    }
    let mut distance_frame_count = 0usize;
    for (start, end) in records.frames(ctx, distance_record_index)? {
        if end.checked_sub(start) == Some(104)
            && distance_frame_matches(bytes, start, distance_record_index, scope.record_index)
        {
            distance_frame_count += 1;
        }
    }
    if distance_frame_count != 1 {
        return Ok(None);
    }
    let mut candidate = None;
    for (start, end) in records.frames(ctx, boundary_record_index)? {
        let Some(tail) = boundary_frame_tail(
            bytes,
            start,
            end,
            edge_count,
            boundary_kind,
            boundary_record_index,
            scope.record_index,
        ) else {
            continue;
        };
        // The frame lists the edges in reference-table order after its header.
        let mut ordinal = 0usize;
        let mismatch = reference_position(
            ctx,
            references,
            |record_index| {
                let Some(edge) = ordinal.checked_sub(2) else {
                    ordinal += 1;
                    return Ok(false);
                };
                ordinal += 1;
                Ok(marked_record_reference(bytes, start + 25 + edge * 11) != Some(*record_index))
            },
            "validate F3D surface boundary edge references",
        )?;
        if mismatch.is_none() && candidate.replace(tail).is_some() {
            return Ok(None);
        }
    }
    let Some(tail) = candidate else {
        return Ok(None);
    };
    Ok(Some(ExactSurfaceBoundaryOperation {
        distance: scalar.value,
        distance_offset: scalar.value_offset,
        distance_record_index,
        mode: tail.mode,
        mode_offset: tail.mode_offset,
        boundary_record_index,
        boundary_reference_record_index: tail.boundary_reference_record_index,
        boundary_reference_offset: tail.boundary_reference_offset,
        tolerance: tail.tolerance,
        tolerance_offset: tail.tolerance_offset,
    }))
}

/// Whether the 104-byte distance frame at `start` is owned by the scope and
/// names its neighbouring records.
fn distance_frame_matches(
    bytes: &[u8],
    start: usize,
    distance_record_index: u32,
    scope_record_index: u32,
) -> bool {
    zeros_at::<8>(bytes, start + 11)
        && bytes_at::<5>(bytes, start + 19) == Some(&[1, 1, 0, 0, 0])
        && marked_record_reference(bytes, start + 24) == Some(scope_record_index)
        && zeros_at::<6>(bytes, start + 29)
        && zeros_at::<5>(bytes, start + 35)
        && marked_record_reference(bytes, start + 48) == distance_record_index.checked_sub(1)
        && zeros_at::<6>(bytes, start + 53)
        && View::u32_le_at(bytes, start + 59).is_some_and(|value| value != 0)
        && zeros_at::<4>(bytes, start + 63)
        && marked_record_reference(bytes, start + 67) == Some(scope_record_index)
        && zeros_at::<6>(bytes, start + 72)
        && bytes_at::<3>(bytes, start + 78) == Some(&[1, 0, 0])
        && marked_record_reference(bytes, start + 81) == distance_record_index.checked_add(1)
        && zeros_at::<7>(bytes, start + 86)
        && marked_record_reference(bytes, start + 93) == Some(scope_record_index)
        && zeros_at::<6>(bytes, start + 98)
}

struct BoundaryFrameTail {
    mode: u32,
    mode_offset: u64,
    boundary_reference_record_index: u32,
    boundary_reference_offset: u64,
    tolerance: PositiveReal,
    tolerance_offset: u64,
}

/// The fixed fields of the boundary frame from `start` to `end` that follow
/// its `edge_count` edge references.
fn boundary_frame_tail(
    bytes: &[u8],
    start: usize,
    end: usize,
    edge_count: usize,
    boundary_kind: u32,
    boundary_record_index: u32,
    scope_record_index: u32,
) -> Option<BoundaryFrameTail> {
    let member_bytes = edge_count.checked_mul(11)?;
    let tail = start.checked_add(25)?.checked_add(member_bytes)?;
    if end.checked_sub(start)? != 113usize.checked_add(member_bytes)?
        || !zeros_at::<10>(bytes, start + 11)
        || View::u32_le_at(bytes, start + 21)? != u32::try_from(edge_count).ok()?
        || !zeros_at::<2>(bytes, tail)
        || !zeros_at::<10>(bytes, tail + 11)
        || View::u32_le_at(bytes, tail + 21)? != boundary_kind
        || !zeros_at::<10>(bytes, tail + 25)
        || View::u32_le_at(bytes, tail + 35)? != 210
        || View::u32_le_at(bytes, tail + 47)? != 210
        || marked_record_reference(bytes, tail + 51) != boundary_record_index.checked_add(2)
        || !zeros_at::<6>(bytes, tail + 56)
        || bytes_at::<3>(bytes, tail + 62)? != &[1, 0, 0]
        || marked_record_reference(bytes, tail + 65) != boundary_record_index.checked_add(1)
        || !zeros_at::<7>(bytes, tail + 70)
        || marked_record_reference(bytes, tail + 77) != Some(scope_record_index)
        || !zeros_at::<6>(bytes, tail + 82)
    {
        return None;
    }
    Some(BoundaryFrameTail {
        mode: View::u32_le_at(bytes, tail + 2)?,
        mode_offset: u64::try_from(tail + 2).ok()?,
        boundary_reference_record_index: marked_record_reference(bytes, tail + 6)?,
        boundary_reference_offset: u64::try_from(tail + 6).ok()?,
        tolerance: PositiveReal::new(View::f64_le_at(bytes, tail + 39)?)?,
        tolerance_offset: u64::try_from(tail + 39).ok()?,
    })
}

/// The boundary edges: the scope's references after the distance owner and
/// the boundary record.
fn boundary_edge_record_indices(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
) -> Result<Vec<u32>, CodecError> {
    let references = scope.reference_members();
    let mut edges = ctx.collection_vec(
        references.len().saturating_sub(2),
        "f3d surface boundary edges",
    )?;
    edges.extend(
        admit_reference_values(ctx, references, "copy F3D surface boundary edges")?
            .skip(2)
            .copied(),
    );
    Ok(edges)
}

pub(super) fn exact_surface_stitch_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    references: &[u32],
) -> Result<Option<DesignSurfaceStitchOperation>, CodecError> {
    let [.., tolerance_record_index, settings_record_index] = references else {
        return Ok(None);
    };
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return Ok(None);
    }
    let Some(scalar) = exact_fixed_scalar(ctx, bytes, records, *tolerance_record_index)? else {
        return Ok(None);
    };
    if scalar.owner_record_index != Some(scope_record_index) || scalar.ordinal != 0 {
        return Ok(None);
    }
    let Some(gap_tolerance) = PositiveReal::new(scalar.value.get()) else {
        return Ok(None);
    };
    Ok(Some(DesignSurfaceStitchOperation {
        gap_tolerance,
        gap_tolerance_offset: scalar.value_offset,
        tolerance_record_index: *tolerance_record_index,
        settings_record_index: *settings_record_index,
    }))
}

/// The direction entity identity that marks an absent direction.
const ABSENT_DIRECTION_ENTITY: [u8; 36] = *b"00000000-0000-0000-0000-000000000000";

/// Largest record count of one ruled-surface reference list.
const MAX_RULED_SURFACE_REFERENCES: usize = 100_000;

pub(super) fn exact_ruled_surface_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Result<Option<DesignRuledSurfaceOperation>, CodecError> {
    let Some(prologue) = ruled_surface_prologue(bytes, start) else {
        return Ok(None);
    };
    // The distance and angle owners lead the ordered reference table.
    if reference_members.first() != Some(&prologue.distance_owner_record_index)
        || reference_members.get(1) != Some(&prologue.angle_owner_record_index)
    {
        return Ok(None);
    }
    let Some((mut edge_group_record_indices, cursor)) =
        ruled_surface_reference_list(ctx, bytes, start.saturating_add(54))?
    else {
        return Ok(None);
    };
    if View::u32_le_at(bytes, cursor) != Some(0) {
        return Ok(None);
    }
    let Some((auxiliary_record_indices, cursor)) =
        ruled_surface_reference_list(ctx, bytes, cursor.saturating_add(4))?
    else {
        return Ok(None);
    };
    if View::u32_le_at(bytes, cursor) != Some(0) {
        return Ok(None);
    }
    let Some((trailing_edge_groups, cursor)) =
        ruled_surface_reference_list(ctx, bytes, cursor.saturating_add(4))?
    else {
        return Ok(None);
    };
    ctx.extend_vec(
        &mut edge_group_record_indices,
        trailing_edge_groups,
        "f3d ruled surface merged edge groups",
    )?;
    if edge_group_record_indices.is_empty() {
        return Ok(None);
    }
    let Some((direction_entity, direction_end)) = fixed_guid_ascii(ctx, bytes, cursor)? else {
        return Ok(None);
    };
    if direction_end.checked_add(3) != Some(reference_count_at)
        || !zeros_at::<3>(bytes, direction_end)
        || paired_at <= reference_count_at
    {
        return Ok(None);
    }
    if !ctx.all_by(
        &edge_group_record_indices,
        |record_index| {
            ctx.contains(
                reference_members,
                record_index,
                "find F3D ruled surface listed edge group",
            )
        },
        "validate F3D ruled surface edge groups",
    )? {
        return Ok(None);
    }
    let direction_entity_id = if direction_entity == ABSENT_DIRECTION_ENTITY {
        None
    } else {
        let text = std::str::from_utf8(&direction_entity)
            .map_err(|_| CodecError::malformed("validated F3D relaxed GUID is not ASCII"))?;
        Some(
            DesignRelaxedGuidText::try_from(
                ctx.copy_retained_text(text, "retain F3D ruled surface direction entity")?,
            )
            .map_err(CodecError::malformed)?,
        )
    };
    Ok(Some(DesignRuledSurfaceOperation {
        method: prologue.method,
        method_offset: u64_from_index(prologue.method_offset),
        corner: prologue.corner,
        corner_offset: u64_from_index(prologue.corner_offset),
        alternate_face: prologue.alternate_face,
        alternate_face_offset: u64_from_index(prologue.alternate_face_offset),
        angle_owner_record_index: prologue.angle_owner_record_index,
        distance_owner_record_index: prologue.distance_owner_record_index,
        edge_group_record_indices,
        auxiliary_record_indices,
        direction_entity_id,
    }))
}

/// The fixed fields before the reference lists of a `SurfaceRuled` frame.
struct RuledSurfacePrologue {
    method: DesignRuledSurfaceMethod,
    method_offset: usize,
    alternate_face: bool,
    alternate_face_offset: usize,
    angle_owner_record_index: u32,
    distance_owner_record_index: u32,
    corner: DesignRuledSurfaceCorner,
    corner_offset: usize,
}

fn ruled_surface_prologue(bytes: &[u8], start: usize) -> Option<RuledSurfacePrologue> {
    if !zeros_at::<9>(bytes, start.checked_add(11)?) {
        return None;
    }
    let method_offset = start.checked_add(20)?;
    let method = match View::u32_le_at(bytes, method_offset)? {
        0 => DesignRuledSurfaceMethod::Tangent,
        1 => DesignRuledSurfaceMethod::Normal,
        2 => DesignRuledSurfaceMethod::Direction,
        _ => return None,
    };
    if !zeros_at::<3>(bytes, start.checked_add(24)?) {
        return None;
    }
    let alternate_face_offset = start.checked_add(27)?;
    let alternate_face = match bytes.get(alternate_face_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let (angle_owner_record_index, _) =
        exact_same_segment_record_reference(bytes, start.checked_add(28)?)?;
    let (distance_owner_record_index, _) =
        exact_same_segment_record_reference(bytes, start.checked_add(39)?)?;
    let corner_offset = start.checked_add(50)?;
    let corner = match View::u32_le_at(bytes, corner_offset)? {
        0 => DesignRuledSurfaceCorner::Rounded,
        1 => DesignRuledSurfaceCorner::Mitered,
        _ => return None,
    };
    Some(RuledSurfacePrologue {
        method,
        method_offset,
        alternate_face,
        alternate_face_offset,
        angle_owner_record_index,
        distance_owner_record_index,
        corner,
        corner_offset,
    })
}

/// The counted list of eleven-byte record references at `count_at` and the
/// offset after it. A count the bytes cannot supply is no list.
fn ruled_surface_reference_list(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    count_at: usize,
) -> Result<Option<(Vec<u32>, usize)>, CodecError> {
    let Some(count) =
        View::u32_le_at(bytes, count_at).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    let Some(first) = count_at.checked_add(4) else {
        return Ok(None);
    };
    let Some(end) = count
        .checked_mul(11)
        .and_then(|length| first.checked_add(length))
    else {
        return Ok(None);
    };
    if count > MAX_RULED_SURFACE_REFERENCES || end > bytes.len() {
        return Ok(None);
    }
    let mut records = ctx.collection_vec(count, "f3d ruled surface references")?;
    for ordinal in ctx.admit_iter(&(0..count), "read F3D ruled surface references")? {
        let Some((record_index, _)) =
            exact_same_segment_record_reference(bytes, first + ordinal * 11)
        else {
            return Ok(None);
        };
        records.push(record_index);
    }
    Ok(Some((records, end)))
}
