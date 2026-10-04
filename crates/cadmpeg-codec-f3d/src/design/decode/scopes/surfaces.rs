// SPDX-License-Identifier: Apache-2.0
//! Exact surface extend, offset, boundary, stitch and ruled-surface operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::take_reference;
use crate::design::decode::operands::parse_construction_operand_group;
use crate::design::decode::operands::ConstructionOperandGroupParse;
use crate::design::decode::operands::RecordFrame;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignRuledSurfaceCorner;
use crate::records::feature::surface_ops::DesignRuledSurfaceMethod;
use crate::records::feature::surface_ops::DesignRuledSurfaceOperation;
use crate::records::feature::surface_ops::DesignSurfaceExtendMethod;
use crate::records::feature::surface_ops::DesignSurfaceExtendOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetOperation;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;
use crate::records::feature::surface_ops::DesignSurfaceStitchOperation;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashSet;

pub(super) fn exact_surface_extend_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceExtendOperation>, CodecError> {
    let Some(operation) = exact_surface_boundary_operation(
        ctx,
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceExtend,
        8,
    )?
    else {
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
        edge_record_indices: operation.edge_record_indices,
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
    if let Some(operation) = exact_surface_offset_face_groups(ctx, bytes, records, scope)? {
        return Ok(Some(operation));
    }
    let Some(operation) = exact_surface_boundary_operation(
        ctx,
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceOffset,
        65,
    )?
    else {
        return Ok(None);
    };
    Ok(
        (operation.mode == 1).then_some(DesignSurfaceOffsetOperation {
            distance: operation.distance,
            distance_offset: operation.distance_offset,
            distance_record_index: operation.distance_record_index,
            support: DesignSurfaceOffsetSupport::BoundaryCarrier {
                boundary_record_index: operation.boundary_record_index,
                boundary_reference_record_index: operation.boundary_reference_record_index,
                boundary_reference_offset: operation.boundary_reference_offset,
                edge_record_indices: operation.edge_record_indices,
                tolerance: operation.tolerance,
                tolerance_offset: operation.tolerance_offset,
            },
        }),
    )
}

fn exact_surface_offset_face_groups(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignSurfaceOffsetOperation>, CodecError> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::SurfaceOffset) {
            return None;
        }
        let distance_record_index = scope.reference_members().values().next()?;
        let support_reference_count = scope.reference_members().len() - 1;
        if support_reference_count == 0 {
            return None;
        }
        let scalar = match exact_fixed_scalar(ctx, bytes, records, *distance_record_index) {
            Ok(Some(scalar)) => scalar,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
            return None;
        }

        let mut group_record_indices = Vec::new();
        let mut covered_references = HashSet::new();
        let support_references = match super::parameter_scope::reference_members(
            ctx,
            scope.reference_members(),
            "scan F3D surface offset support references",
        ) {
            Ok(references) => references,
            Err(error) => return Some(Err(error)),
        };
        for (scope_reference_ordinal, record_index) in support_references.enumerate().skip(1) {
            let group = exact_construction_operand_group(
                ctx,
                bytes,
                records,
                scope,
                u32::try_from(scope_reference_ordinal).ok()?,
                record_index,
            );
            let Some(group) = group else {
                continue;
            };
            let group = match group {
                Ok(group) => group,
                Err(error) => return Some(Err(error)),
            };
            if group.role() != DesignOperandRole::PROFILE
                || group.frame.opaque_index.get() != 252
                || group.members().is_empty()
                || covered_references.contains(&group.record_index)
            {
                return None;
            }

            if let Err(error) = ctx.reserve_set(
                &mut covered_references,
                1,
                "f3d surface offset covered reference",
            ) {
                return Some(Err(error));
            }
            covered_references.insert(group.record_index);
            let group_members =
                match ctx.admit_iter(group.members(), "scan F3D surface offset group members") {
                    Ok(members) => members.map(|member| &member.value),
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
            for member in group_members {
                if *member == *distance_record_index {
                    return None;
                }
                let is_support_reference = match super::parameter_scope::reference_members(
                    ctx,
                    scope.reference_members(),
                    "check F3D surface offset support references",
                ) {
                    Ok(references) => references.skip(1).any(|value| value == *member),
                    Err(error) => return Some(Err(error)),
                };
                if !is_support_reference || covered_references.contains(member) {
                    return None;
                }

                if let Err(error) = ctx.reserve_set(
                    &mut covered_references,
                    1,
                    "f3d surface offset covered reference",
                ) {
                    return Some(Err(error));
                }
                covered_references.insert(*member);
            }

            if let Err(error) = ctx.reserve_capacity(
                &mut group_record_indices,
                1,
                "f3d surface offset face group",
            ) {
                return Some(Err(error));
            }
            if let Err(error) = ctx.push_vec(
                &mut group_record_indices,
                group.record_index,
                "f3d surface offset face group",
            ) {
                return Some(Err(error));
            };
        }
        if group_record_indices.is_empty() || covered_references.len() != support_reference_count {
            return None;
        }

        Some(Ok(DesignSurfaceOffsetOperation {
            distance: scalar.value,
            distance_offset: scalar.value_offset,
            distance_record_index: *distance_record_index,
            support: DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices,
            },
        }))
    })();
    parsed.transpose()
}

fn exact_construction_operand_group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    record_index: u32,
) -> Option<
    Result<crate::records::topology::construction::DesignConstructionOperandGroup, CodecError>,
> {
    let mut candidate = None;
    let frames = match records.frames(ctx, record_index) {
        Ok(frames) => frames,
        Err(error) => return Some(Err(error)),
    };
    for (start, _) in frames {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if after_tag != start + 7 {
            continue;
        }
        let class_tag = match crate::design::decode::text::class_tag_from_view(ctx, class_tag) {
            Ok(Some(class_tag)) => class_tag,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let header = RecordFrame {
            record_index,
            class_tag,
            byte_offset: u64::try_from(start).ok()?,
        };
        match parse_construction_operand_group(ctx, bytes, scope, scope_reference_ordinal, &header)
        {
            ConstructionOperandGroupParse::Complete(group) => {
                if candidate.replace(*group).is_some() {
                    return None;
                }
            }
            ConstructionOperandGroupParse::Refused(error) => return Some(Err(error)),
            ConstructionOperandGroupParse::NotAGroup | ConstructionOperandGroupParse::Unclosed => {}
        }
    }
    candidate.map(Ok)
}

#[derive(Clone)]
struct ExactSurfaceBoundaryOperation {
    distance: cadmpeg_ir::scalar::FiniteReal,
    distance_offset: u64,
    distance_record_index: u32,
    mode: u32,
    mode_offset: u64,
    boundary_record_index: u32,
    boundary_reference_record_index: u32,
    boundary_reference_offset: u64,
    edge_record_indices: Vec<u32>,
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    tolerance_offset: u64,
}

fn exact_surface_boundary_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    family: DesignFeatureFamily,
    boundary_kind: u32,
) -> Result<Option<ExactSurfaceBoundaryOperation>, CodecError> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(family) {
            return None;
        }
        let mut references = scope.reference_members().values();
        let distance_record_index = references.next()?;
        let boundary_record_index = references.next()?;
        let edge_record_indices = references;
        if edge_record_indices.len() == 0 {
            return None;
        }
        let scalar = match exact_fixed_scalar(ctx, bytes, records, *distance_record_index) {
            Ok(Some(scalar)) => scalar,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
            return None;
        }
        let distance_frames = match records.frames(ctx, *distance_record_index) {
            Ok(frames) => frames,
            Err(error) => return Some(Err(error)),
        };
        let mut distance_frame_count = 0usize;
        for (start, end) in distance_frames {
            if end.checked_sub(start) != Some(104) {
                continue;
            }
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if after_tag != start + 7 || class_tag.len() != 3 {
                continue;
            }
            let class_tag_is_digits = match ctx.admit_iter(
                class_tag.as_bytes(),
                "validate F3D surface offset distance class tag",
            ) {
                Ok(mut bytes) => bytes.all(u8::is_ascii_digit),
                Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
            };
            if !class_tag_is_digits {
                continue;
            }
            if bytes.get(start + 11..start + 19) == Some(&[0; 8])
                && bytes.get(start + 19..start + 24) == Some(&[1, 1, 0, 0, 0])
                && marked_record_reference(bytes, start + 24) == Some(scope.record_index)
                && bytes.get(start + 29..start + 35) == Some(&[0; 6])
                && bytes.get(start + 35..start + 40) == Some(&[0; 5])
                && marked_record_reference(bytes, start + 48)
                    == distance_record_index.checked_sub(1)
                && bytes.get(start + 53..start + 59) == Some(&[0; 6])
                && View::u32_le_at(bytes, start + 59).is_some_and(|value| value != 0)
                && bytes.get(start + 63..start + 67) == Some(&[0; 4])
                && marked_record_reference(bytes, start + 67) == Some(scope.record_index)
                && bytes.get(start + 72..start + 78) == Some(&[0; 6])
                && bytes.get(start + 78..start + 81) == Some(&[1, 0, 0])
                && marked_record_reference(bytes, start + 81)
                    == distance_record_index.checked_add(1)
                && bytes.get(start + 86..start + 93) == Some(&[0; 7])
                && marked_record_reference(bytes, start + 93) == Some(scope.record_index)
                && bytes.get(start + 98..start + 104) == Some(&[0; 6])
            {
                distance_frame_count += 1;
            }
        }
        if distance_frame_count != 1 {
            return None;
        }
        let mut candidate = None;
        let boundary_frames = match records.frames(ctx, *boundary_record_index) {
            Ok(frames) => frames,
            Err(error) => return Some(Err(error)),
        };
        for (start, end) in boundary_frames {
            let parsed = (|| {
                let member_bytes = edge_record_indices.len().checked_mul(11)?;
                let tail = start.checked_add(25)?.checked_add(member_bytes)?;
                (end.checked_sub(start)? == 113usize.checked_add(member_bytes)?).then_some(())?;
                let (class_tag, after_tag) =
                    lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
                if after_tag != start + 7 || class_tag.len() != 3 {
                    return None;
                }
                let class_tag_is_digits = match ctx.admit_iter(
                    class_tag.as_bytes(),
                    "validate F3D surface boundary class tag",
                ) {
                    Ok(mut bytes) => bytes.all(u8::is_ascii_digit),
                    Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
                };
                if !class_tag_is_digits
                    || bytes.get(start + 11..start + 21)? != [0; 10]
                    || View::u32_le_at(bytes, start + 21)?
                        != u32::try_from(edge_record_indices.len()).ok()?
                {
                    return None;
                }
                let has_wrong_edge_reference = match super::parameter_scope::reference_members(
                    ctx,
                    scope.reference_members(),
                    "validate F3D surface boundary edge references",
                ) {
                    Ok(references) => {
                        references
                            .skip(2)
                            .enumerate()
                            .any(|(ordinal, record_index)| {
                                marked_record_reference(bytes, start + 25 + ordinal * 11)
                                    != Some(record_index)
                            })
                    }
                    Err(error) => return Some(Err(error)),
                };
                if has_wrong_edge_reference
                    || bytes.get(tail..tail + 2)? != [0; 2]
                    || bytes.get(tail + 11..tail + 21)? != [0; 10]
                    || View::u32_le_at(bytes, tail + 21)? != boundary_kind
                    || bytes.get(tail + 25..tail + 35)? != [0; 10]
                    || View::u32_le_at(bytes, tail + 35)? != 210
                    || View::u32_le_at(bytes, tail + 47)? != 210
                    || marked_record_reference(bytes, tail + 51)
                        != boundary_record_index.checked_add(2)
                    || bytes.get(tail + 56..tail + 62)? != [0; 6]
                    || bytes.get(tail + 62..tail + 65)? != [1, 0, 0]
                    || marked_record_reference(bytes, tail + 65)
                        != boundary_record_index.checked_add(1)
                    || bytes.get(tail + 70..tail + 77)? != [0; 7]
                    || marked_record_reference(bytes, tail + 77) != Some(scope.record_index)
                    || bytes.get(tail + 82..tail + 88)? != [0; 6]
                {
                    return None;
                }
                let mode = View::u32_le_at(bytes, tail + 2)?;
                let boundary_reference_record_index = marked_record_reference(bytes, tail + 6)?;
                let tolerance =
                    cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, tail + 39)?)?;
                Some(Ok((
                    mode,
                    u64::try_from(tail + 2).ok()?,
                    boundary_reference_record_index,
                    u64::try_from(tail + 6).ok()?,
                    tolerance,
                    u64::try_from(tail + 39).ok()?,
                )))
            })();
            match parsed {
                Some(Ok(parsed)) => {
                    if candidate.replace(parsed).is_some() {
                        return None;
                    }
                }
                Some(Err(error)) => return Some(Err(error)),
                None => {}
            }
        }
        let (
            mode,
            mode_offset,
            boundary_reference_record_index,
            boundary_reference_offset,
            tolerance,
            tolerance_offset,
        ) = candidate?;
        Some(Ok((
            scalar,
            *distance_record_index,
            *boundary_record_index,
            edge_record_indices,
            mode,
            mode_offset,
            boundary_reference_record_index,
            boundary_reference_offset,
            tolerance,
            tolerance_offset,
        )))
    })();
    let parsed = parsed.transpose()?;
    let Some((
        scalar,
        distance_record_index,
        boundary_record_index,
        edge_record_indices,
        mode,
        mode_offset,
        boundary_reference_record_index,
        boundary_reference_offset,
        tolerance,
        tolerance_offset,
    )) = parsed
    else {
        return Ok(None);
    };

    let mut edges = Vec::new();
    ctx.reserve_vec(
        &mut edges,
        edge_record_indices.len(),
        "f3d surface boundary edges",
    )?;
    edges.extend(edge_record_indices.copied());
    Ok(Some(ExactSurfaceBoundaryOperation {
        distance: scalar.value,
        distance_offset: scalar.value_offset,
        distance_record_index,
        mode,
        mode_offset,
        boundary_record_index,
        boundary_reference_record_index,
        boundary_reference_offset,
        edge_record_indices: edges,
        tolerance,
        tolerance_offset,
    }))
}

pub(super) fn exact_surface_stitch_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    references: &[u32],
) -> Result<Option<DesignSurfaceStitchOperation>, CodecError> {
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return Ok(None);
    }
    let tolerance_record_index = references[references.len() - 2];
    let settings_record_index = references[references.len() - 1];
    let scalar = match exact_fixed_scalar(ctx, bytes, records, tolerance_record_index) {
        Ok(Some(scalar)) => scalar,
        Ok(None) => return Ok(None),
        Err(error) => return Err(error),
    };
    if scalar.owner_record_index != Some(scope_record_index) || scalar.ordinal != 0 {
        return Ok(None);
    }
    let Some(gap_tolerance) = cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get()) else {
        return Ok(None);
    };
    Ok(Some(DesignSurfaceStitchOperation {
        gap_tolerance,
        gap_tolerance_offset: scalar.value_offset,
        tolerance_record_index,
        settings_record_index,
    }))
}

pub(super) fn exact_ruled_surface_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Result<Option<DesignRuledSurfaceOperation>, CodecError> {
    let parsed = (|| {
        if bytes.get(start.checked_add(11)?..start.checked_add(20)?)? != [0; 9] {
            return None;
        }
        let method_offset = start.checked_add(20)?;
        let method = match View::u32_le_at(bytes, method_offset)? {
            0 => DesignRuledSurfaceMethod::Tangent,
            1 => DesignRuledSurfaceMethod::Normal,
            2 => DesignRuledSurfaceMethod::Direction,
            _ => return None,
        };
        if bytes.get(start.checked_add(24)?..start.checked_add(27)?)? != [0; 3] {
            return None;
        }
        let alternate_face_offset = start.checked_add(27)?;
        let alternate_face = match bytes.get(alternate_face_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let fixed_reference = |at: usize| {
            let mut cursor = at;
            let reference = take_reference(bytes, &mut cursor)?;
            (cursor == at.checked_add(11)?).then(|| u32::try_from(reference.local()?.0).ok())?
        };
        let angle_owner_record_index = fixed_reference(start.checked_add(28)?)?;
        let distance_owner_record_index = fixed_reference(start.checked_add(39)?)?;
        let corner_offset = start.checked_add(50)?;
        let corner = match View::u32_le_at(bytes, corner_offset)? {
            0 => DesignRuledSurfaceCorner::Rounded,
            1 => DesignRuledSurfaceCorner::Mitered,
            _ => return None,
        };
        let take_reference_list = |mut cursor: usize| {
            let count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
            if count > 100_000 {
                return None;
            }
            cursor = cursor.checked_add(4)?;

            let mut records = Vec::new();
            if let Err(error) =
                ctx.reserve_capacity(&mut records, count, "f3d ruled surface references")
            {
                return Some(Err(error));
            }
            for _ in 0..count {
                if let Err(error) = ctx.push_vec(
                    &mut records,
                    match fixed_reference(cursor) {
                        Some(record) => record,
                        None => return None,
                    },
                    "f3d ruled surface references",
                ) {
                    return Some(Err(error));
                };
                cursor = cursor.checked_add(11)?;
            }
            Some(Ok((records, cursor)))
        };
        let (mut edge_group_record_indices, mut cursor) =
            match take_reference_list(start.checked_add(54)?)? {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
        if View::u32_le_at(bytes, cursor)? != 0 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let (auxiliary_record_indices, next) = match take_reference_list(cursor)? {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        cursor = next;
        if View::u32_le_at(bytes, cursor)? != 0 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let (trailing_edge_groups, next) = match take_reference_list(cursor)? {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        cursor = next;

        if let Err(error) = ctx.reserve_vec(
            &mut edge_group_record_indices,
            trailing_edge_groups.len(),
            "f3d ruled surface merged edge groups",
        ) {
            return Some(Err(error));
        }
        edge_group_record_indices.extend(trailing_edge_groups);
        let (direction_entity_id, direction_end) = match fixed_relaxed_guid_text(ctx, bytes, cursor)
        {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let direction_absent =
            direction_entity_id.as_str() == "00000000-0000-0000-0000-000000000000";
        if direction_end.checked_add(3)? != reference_count_at
            || bytes.get(direction_end..reference_count_at)? != [0; 3]
            || paired_at <= reference_count_at
        {
            return None;
        }
        let direction_entity_id = if direction_absent {
            None
        } else {
            Some(direction_entity_id)
        };
        if reference_members.first() != Some(&distance_owner_record_index)
            || reference_members.get(1) != Some(&angle_owner_record_index)
            || edge_group_record_indices.is_empty()
        {
            return None;
        }
        let has_unlisted_edge_group = match ctx.admit_iter(
            &edge_group_record_indices,
            "validate F3D ruled surface edge groups",
        ) {
            Ok(groups) => {
                let mut unlisted = false;
                for record_index in groups {
                    match ctx.contains(
                        reference_members,
                        record_index,
                        "find F3D ruled surface listed edge group",
                    ) {
                        Ok(true) => {}
                        Ok(false) => {
                            unlisted = true;
                            break;
                        }
                        Err(error) => return Some(Err(error)),
                    }
                }
                unlisted
            }
            Err(error) => return Some(Err(cadmpeg_core::CodecError::ResourceLimit(error))),
        };
        if has_unlisted_edge_group {
            return None;
        }
        Some(Ok(DesignRuledSurfaceOperation {
            method,
            method_offset: u64_from_index(method_offset),
            corner,
            corner_offset: u64_from_index(corner_offset),
            alternate_face,
            alternate_face_offset: u64_from_index(alternate_face_offset),
            angle_owner_record_index,
            distance_owner_record_index,
            edge_group_record_indices,
            auxiliary_record_indices,
            direction_entity_id,
        }))
    })();
    parsed.transpose()
}
