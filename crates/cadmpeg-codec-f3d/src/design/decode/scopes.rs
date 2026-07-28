// SPDX-License-Identifier: Apache-2.0
//! Parse parameter scopes and exact feature-construction frames.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::design::decode::sketch::{
    next_indexed_record_offset, next_indexed_record_offset_with_index, valid_sketch_transform,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::records::{
    DesignBaseFeatureConstruction, DesignBaseFlangeOperation, DesignCircularPatternConstruction,
    DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement,
    DesignCopyPasteBodiesOperation, DesignDirectFaceOperation, DesignEdgeFlangeOperation,
    DesignEntityHeader, DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudeStart,
    DesignFixedChamferParameters, DesignFixedExtrudeParameters, DesignFixedFilletParameters,
    DesignHemOperation, DesignMoveOperation, DesignObjectKind, DesignParameterScope,
    DesignPathFeatureConstruction, DesignRecordHeader, DesignScaleOperation, DesignSolidPrimitive,
    DesignSurfaceStitchOperation,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{f64_at, f64s_at, u32_at, u64_at as read_u64};
use std::collections::{HashMap, HashSet};

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub fn decode_parameter_scopes(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignParameterScope>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        for header in parameter_scope_candidate_headers(bytes) {
            let Some(mut scope) = parse_parameter_scope(bytes, &header) else {
                continue;
            };
            if design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sketch) {
                let start = usize::try_from(scope.byte_offset).ok();
                let end = usize::try_from(scope.paired_byte_offset).ok();
                let frame = start
                    .zip(end)
                    .and_then(|(start, end)| bytes.get(start..end));
                let matches = frame
                    .into_iter()
                    .flat_map(|frame| {
                        entities.iter().filter_map(|entity| {
                            if native_stream(&entity.id) != Some(stream.as_str())
                                || entity.object_kind != Some(DesignObjectKind::Sketch)
                                || entity.entity_suffix > u64::from(u32::MAX)
                            {
                                return None;
                            }
                            let mut pattern = [0; 11];
                            pattern[0] = 1;
                            pattern[1..5]
                                .copy_from_slice(&(entity.entity_suffix as u32).to_le_bytes());
                            frame
                                .windows(pattern.len())
                                .position(|window| window == pattern)
                                .map(|offset| (entity, offset + 1))
                        })
                    })
                    .collect::<Vec<_>>();
                if let [(entity, relative_offset)] = matches.as_slice() {
                    scope.entity_id = Some(entity.entity_id.clone());
                    scope.entity_suffix = Some(entity.entity_suffix);
                    scope.entity_reference_offset =
                        Some(scope.byte_offset.saturating_add(*relative_offset as u64));
                }
            }
            if scope.kind == "WorkPlane" {
                if let Some(frame) = exact_work_plane_frame(bytes, &scope) {
                    scope.work_plane_transform = Some(frame.transform);
                    scope.work_plane_transform_offset = Some(frame.transform_offset);
                    if let Some((reference, reference_offset)) = frame.reference {
                        scope.work_plane_reference = Some(reference);
                        scope.work_plane_reference_offset = Some(reference_offset);
                    }
                }
            }
            if scope.kind == "JointOrigin" {
                if let Some(frame) = exact_joint_origin_frame(bytes, &scope) {
                    scope.joint_origin_transform = Some(frame.transform);
                    scope.joint_origin_transform_offset = Some(frame.transform_offset);
                    if let Some((reference, reference_offset)) = frame.reference {
                        scope.joint_origin_reference = Some(reference);
                        scope.joint_origin_reference_offset = Some(reference_offset);
                    }
                }
            }
            if let Some((position, offset)) = exact_work_point_position(bytes, &scope) {
                scope.work_point_position = Some(position);
                scope.work_point_position_offset = Some(offset);
            }
            scope.solid_primitive = exact_solid_primitive(bytes, &scope);
            scope.direct_face_operation = exact_direct_face_operation(bytes, &scope);
            scope.move_operation = exact_move_operation(bytes, &scope);
            scope.scale_operation = exact_scale_operation(bytes, &scope);
            scope.fixed_extrude_parameters = exact_fixed_extrude_parameters(bytes, &scope);
            scope.fixed_fillet_parameters = exact_fixed_fillet_parameters(bytes, &scope);
            scope.fixed_chamfer_parameters = exact_fixed_chamfer_parameters(bytes, &scope);
            scope.path_feature_construction = exact_path_feature_construction(bytes, &scope);
            scope.circular_pattern_construction =
                exact_circular_pattern_construction(bytes, &scope);
            scope.copy_paste_bodies_operation = exact_copy_paste_bodies_operation(bytes, &scope);
            scope.base_feature_construction = exact_base_feature_construction(bytes, &scope);
            scope.id = ids::native_design_parameter_scope_id(&entry.name, scope.byte_offset);
            out.push(scope);
        }
    }
    out.sort_by_key(|scope| scope.id.clone());
    out.dedup_by_key(|scope| scope.id.clone());
    Ok(out)
}

pub(crate) fn exact_circular_pattern_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignCircularPatternConstruction> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::CircularPattern) {
        return None;
    }
    let mut axis_candidates = Vec::new();
    for pair in scope.reference_members.windows(2) {
        let [record_index, selection_record_index] = pair else {
            continue;
        };
        for (start, paired_at) in indexed_record_pairs(bytes, *record_index) {
            if let Some((origin, direction)) = exact_circular_pattern_axis(
                bytes,
                start,
                paired_at,
                *record_index,
                *selection_record_index,
                scope.record_index,
            ) {
                axis_candidates.push((
                    origin,
                    (start + 25) as u64,
                    direction,
                    (start + 49) as u64,
                    *record_index,
                    *selection_record_index,
                ));
            }
        }
    }
    let [(origin, origin_offset, direction, direction_offset, record_index, selection_record_index)] =
        axis_candidates.as_slice()
    else {
        return None;
    };
    let count_candidates = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            exact_fixed_pattern_count(bytes, *record_index, scope.record_index)
                .map(|(count, count_offset)| (count, *record_index, count_offset))
        })
        .collect::<Vec<_>>();
    let [(count, count_record_index, count_offset)] = count_candidates.as_slice() else {
        return None;
    };
    let angle_candidates = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index)
                && scalar.ordinal == 1
                && scalar.value > 0.0)
                .then_some((scalar.value, *record_index, scalar.value_offset))
        })
        .collect::<Vec<_>>();
    let [(angle, angle_record_index, angle_offset)] = angle_candidates.as_slice() else {
        return None;
    };
    Some(DesignCircularPatternConstruction {
        count: *count,
        count_record_index: *count_record_index,
        count_offset: *count_offset,
        angle: *angle,
        angle_record_index: *angle_record_index,
        angle_offset: *angle_offset,
        origin: *origin,
        origin_offset: *origin_offset,
        direction: *direction,
        direction_offset: *direction_offset,
        axis_record_index: *record_index,
        selection_record_index: *selection_record_index,
    })
}

fn exact_circular_pattern_axis(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    selection_record_index: u32,
    scope_record_index: u32,
) -> Option<([f64; 3], [f64; 3])> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != start + 7
        || u32_at(bytes, after_tag) != Some(record_index)
        || paired_at.checked_sub(start) != Some(195)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || u32_at(bytes, start + 21) != Some(8)
        || bytes.get(start + 73..start + 89) != Some(&[0; 16])
        || u32_at(bytes, start + 89) != Some(9)
        || u32_at(bytes, start + 93) != Some(1)
        || marked_record_reference(bytes, start + 97) != Some(selection_record_index)
        || bytes.get(start + 102..start + 108) != Some(&[0; 6])
        || bytes.get(start + 108..start + 110) != Some(&[0; 2])
        || u32_at(bytes, start + 110) != Some(1)
        || marked_record_reference(bytes, start + 114).is_none()
        || bytes.get(start + 119..start + 125) != Some(&[0; 6])
        || read_u64(bytes, start + 125) != Some(0x0000_0004_0000_0000)
        || bytes.get(start + 133..start + 143) != Some(&[0; 10])
    {
        return None;
    }
    let opaque_index = u32_at(bytes, start + 143)?;
    if opaque_index == 0
        || !f64_at(bytes, start + 147)?.is_finite()
        || u32_at(bytes, start + 155) != Some(opaque_index)
        || marked_record_reference(bytes, start + 159) != record_index.checked_add(2)
        || bytes.get(start + 164..start + 172) != Some(&[0; 8])
        || marked_record_reference(bytes, start + 172) != record_index.checked_add(1)
        || bytes.get(start + 177..start + 184) != Some(&[0; 7])
        || marked_record_reference(bytes, start + 184) != Some(scope_record_index)
        || bytes.get(start + 189..start + 195) != Some(&[0; 6])
    {
        return None;
    }
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag.len() != 3
        || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || paired_after_tag != paired_at + 7
        || u32_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let origin: [f64; 3] = f64s_at(bytes, start + 25, 3)?.try_into().ok()?;
    let direction: [f64; 3] = f64s_at(bytes, start + 49, 3)?.try_into().ok()?;
    let direction_norm = direction
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if origin.iter().any(|coordinate| !coordinate.is_finite())
        || direction.iter().any(|coordinate| !coordinate.is_finite())
        || (direction_norm - 1.0).abs() > 1.0e-12
    {
        return None;
    }
    Some((origin, direction))
}

fn exact_fixed_pattern_count(
    bytes: &[u8],
    record_index: u32,
    scope_record_index: u32,
) -> Option<(u32, u64)> {
    let candidates = indexed_record_pairs(bytes, record_index)
        .into_iter()
        .filter_map(|(start, paired_at)| {
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            if class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || after_tag != start + 7
                || u32_at(bytes, after_tag) != Some(record_index)
                || paired_at.checked_sub(start) != Some(99)
                || bytes.get(start + 11..start + 19) != Some(&[0; 8])
                || bytes.get(start + 19) != Some(&1)
                || u32_at(bytes, start + 20) != Some(1)
                || marked_record_reference(bytes, start + 24) != Some(scope_record_index)
                || bytes.get(start + 29..start + 40) != Some(&[0; 11])
                || marked_record_reference(bytes, start + 44) != record_index.checked_add(2)
                || bytes.get(start + 49..start + 55) != Some(&[0; 6])
                || u32_at(bytes, start + 55)? == 0
                || bytes.get(start + 59..start + 63) != Some(&[0; 4])
                || marked_record_reference(bytes, start + 63) != Some(scope_record_index)
                || bytes.get(start + 68..start + 76) != Some(&[0; 8])
                || marked_record_reference(bytes, start + 76) != record_index.checked_add(1)
                || bytes.get(start + 81..start + 88) != Some(&[0; 7])
                || marked_record_reference(bytes, start + 88) != Some(scope_record_index)
                || bytes.get(start + 93..start + 99) != Some(&[0; 6])
            {
                return None;
            }
            let count = u32_at(bytes, start + 40)?;
            (count > 0).then_some((count, (start + 40) as u64))
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn exact_copy_paste_bodies_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignCopyPasteBodiesOperation> {
    if scope.kind != "CopyPasteBodies" || scope.reference_members.len() < 2 {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let body_group_record_index = marked_record_reference(bytes, start + 29)?;
    let relation_record_index = marked_record_reference(bytes, start + 40)?;
    if scope.reference_members[0] != body_group_record_index {
        return None;
    }
    let search_at = usize::try_from(scope.paired_byte_offset)
        .ok()?
        .checked_add(1)?;
    let body_group_at =
        next_indexed_record_offset_with_index(bytes, search_at, body_group_record_index)?;
    let (body_group_class_tag, body_group_after_tag) =
        lp_ascii_filtered(bytes, body_group_at, 0..=2000, u8::is_ascii_graphic)?;
    let body_group_after_index = body_group_after_tag.checked_add(4)?;
    if bytes.get(body_group_after_index..body_group_after_index + 10)? != [0; 10] {
        return None;
    }
    let body_group_count_at = body_group_after_index.checked_add(10)?;
    let body_group_count = usize::try_from(u32_at(bytes, body_group_count_at)?).ok()?;
    if body_group_count != scope.reference_members.len().checked_sub(1)? {
        return None;
    }
    let mut body_operand_record_indices = Vec::with_capacity(body_group_count);
    let mut body_operand_record_offsets = Vec::with_capacity(body_group_count);
    let mut body_group_cursor = body_group_count_at.checked_add(4)?;
    for expected in &scope.reference_members[1..] {
        let actual = marked_record_reference(bytes, body_group_cursor)?;
        if actual != *expected {
            return None;
        }
        body_operand_record_indices.push(actual);
        body_operand_record_offsets.push(u64::try_from(body_group_cursor + 1).ok()?);
        body_group_cursor = body_group_cursor.checked_add(11)?;
    }
    let relation_at =
        next_indexed_record_offset_with_index(bytes, search_at, relation_record_index)?;
    let (relation_class_tag, after_tag) =
        lp_ascii_filtered(bytes, relation_at, 0..=2000, u8::is_ascii_graphic)?;
    let after_index = after_tag.checked_add(4)?;
    if bytes.get(after_index..after_index + 8)? != [0; 8] {
        return None;
    }
    let count_at = after_index.checked_add(8)?;
    if bytes.get(count_at) != Some(&1) {
        return None;
    }
    let reference_count = usize::try_from(u32_at(bytes, count_at + 1)?).ok()?;
    let body_count = scope.reference_members.len().checked_sub(1)?;
    if reference_count != body_count.checked_mul(2)? {
        return None;
    }
    let mut source_body_entity_suffixes = Vec::with_capacity(body_count);
    let mut source_body_entity_suffix_offsets = Vec::with_capacity(body_count);
    let mut copied_body_entity_suffixes = Vec::with_capacity(body_count);
    let mut copied_body_entity_suffix_offsets = Vec::with_capacity(body_count);
    let references_at = count_at.checked_add(5)?;
    let body_reference = |at: usize, trailing_zeros: usize| {
        if bytes.get(at) != Some(&1)
            || !bytes
                .get(at + 5..at + 5 + trailing_zeros)?
                .iter()
                .all(|byte| *byte == 0)
        {
            return None;
        }
        u32_at(bytes, at + 1)
    };
    for ordinal in 0..body_count {
        let source_at = references_at.checked_add(ordinal.checked_mul(30)?)?;
        let copied_at = source_at.checked_add(15)?;
        source_body_entity_suffixes.push(body_reference(source_at, 10)?);
        source_body_entity_suffix_offsets.push(u64::try_from(source_at + 1).ok()?);
        copied_body_entity_suffixes.push(body_reference(
            copied_at,
            if ordinal + 1 == body_count { 6 } else { 10 },
        )?);
        copied_body_entity_suffix_offsets.push(u64::try_from(copied_at + 1).ok()?);
    }
    if source_body_entity_suffixes
        .iter()
        .chain(&copied_body_entity_suffixes)
        .copied()
        .collect::<HashSet<_>>()
        .len()
        != reference_count
    {
        return None;
    }
    Some(DesignCopyPasteBodiesOperation {
        body_group_record_index,
        body_group_class_tag,
        body_group_byte_offset: u64::try_from(body_group_at).ok()?,
        body_operand_record_indices,
        body_operand_record_offsets,
        relation_record_index,
        relation_class_tag,
        relation_byte_offset: u64::try_from(relation_at).ok()?,
        source_body_entity_suffixes,
        source_body_entity_suffix_offsets,
        copied_body_entity_suffixes,
        copied_body_entity_suffix_offsets,
    })
}

pub(crate) fn exact_base_feature_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignBaseFeatureConstruction> {
    if scope.kind != "Base Feature" {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if scope.frame_length == 267 {
        return Some(DesignBaseFeatureConstruction {
            body_entity_suffixes: Vec::new(),
            body_entity_suffix_offsets: Vec::new(),
            body_entity_fields: Vec::new(),
            body_reference_records: Vec::new(),
            body_reference_record_offsets: Vec::new(),
            body_reference_fields: Vec::new(),
            repeated_reference_fields: Vec::new(),
            metadata_record: u32_at(bytes, usize::try_from(scope.byte_offset).ok()? + 37)?,
            metadata_record_offset: scope.byte_offset + 37,
            metadata_field: bytes.get(start + 45..start + 51)?.try_into().ok()?,
            result_records: Vec::new(),
            result_record_offsets: Vec::new(),
            result_fields: Vec::new(),
        });
    }
    let body_count = scope.frame_length.checked_sub(271)?.checked_div(52)?;
    if body_count == 0 || body_count > 100_000 || scope.frame_length != 271 + body_count * 52 {
        return None;
    }
    let body_count = usize::try_from(body_count).ok()?;
    if bytes.get(start + 19) != Some(&1)
        || u32_at(bytes, start + 20)? != u32::try_from(body_count.checked_mul(2)?).ok()?
    {
        return None;
    }
    let mut cursor = start + 24;
    let mut read_u64_run = |count: usize| {
        let mut values = Vec::with_capacity(count);
        let mut offsets = Vec::with_capacity(count);
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(cursor) != Some(&1) {
                return None;
            }
            values.push(read_u64(bytes, cursor + 1)?);
            offsets.push(u64::try_from(cursor + 1).ok()?);
            fields.push(bytes.get(cursor + 9..cursor + 15)?.try_into().ok()?);
            cursor += 15;
        }
        Some((values, offsets, fields))
    };
    let (body_entity_suffixes, body_entity_suffix_offsets, body_entity_fields) =
        read_u64_run(body_count)?;
    let (body_reference_values, body_reference_record_offsets, body_reference_fields) =
        read_u64_run(body_count)?;
    let body_reference_records = body_reference_values
        .into_iter()
        .map(u32::try_from)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if bytes.get(cursor) != Some(&1) || bytes.get(cursor + 1..cursor + 11) != Some(&[0; 10]) {
        return None;
    }
    cursor += 11;
    if usize::try_from(u32_at(bytes, cursor)?).ok()? != body_count {
        return None;
    }
    cursor += 4;
    let mut repeated_reference_fields = Vec::with_capacity(body_count);
    for expected in &body_reference_records {
        if bytes.get(cursor) != Some(&1) || u32_at(bytes, cursor + 1)? != *expected {
            return None;
        }
        repeated_reference_fields.push(bytes.get(cursor + 5..cursor + 11)?.try_into().ok()?);
        cursor += 11;
    }
    if bytes.get(cursor) != Some(&0) {
        return None;
    }
    cursor += 1;
    if bytes.get(cursor) != Some(&1) {
        return None;
    }
    let metadata_record = u32::try_from(read_u64(bytes, cursor + 1)?).ok()?;
    let metadata_record_offset = u64::try_from(cursor + 1).ok()?;
    let metadata_field = bytes.get(cursor + 9..cursor + 15)?.try_into().ok()?;
    cursor += 15;
    if usize::try_from(u32_at(bytes, cursor)?).ok()? != body_count {
        return None;
    }
    cursor += 4;
    let mut result_records = Vec::with_capacity(body_count);
    let mut result_record_offsets = Vec::with_capacity(body_count);
    let mut result_fields = Vec::with_capacity(body_count);
    for _ in 0..body_count {
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        result_records.push(u32_at(bytes, cursor + 1)?);
        result_record_offsets.push(u64::try_from(cursor + 1).ok()?);
        result_fields.push(bytes.get(cursor + 5..cursor + 11)?.try_into().ok()?);
        cursor += 11;
    }
    let uuid_offset = usize::try_from(scope.kind_offset).ok()?.checked_sub(102)?;
    (cursor <= uuid_offset
        && bytes
            .get(cursor..uuid_offset)
            .is_some_and(|padding| padding.iter().all(|byte| *byte == 0)))
    .then_some(DesignBaseFeatureConstruction {
        body_entity_suffixes,
        body_entity_suffix_offsets,
        body_entity_fields,
        body_reference_records,
        body_reference_record_offsets,
        body_reference_fields,
        repeated_reference_fields,
        metadata_record,
        metadata_record_offset,
        metadata_field,
        result_records,
        result_record_offsets,
        result_fields,
    })
}

pub(crate) fn exact_solid_primitive(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignSolidPrimitive> {
    if !matches!(scope.kind.as_str(), "SpherePrimitive" | "TorusPrimitive") {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let operation_offset = start.checked_add(25)?;
    let operation = match u32_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let matrix = |relative_offset: usize| {
        let matrix_at = start.checked_add(relative_offset)?;
        let values = f64s_at(bytes, matrix_at, 16)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        valid_sketch_transform(&transform).then_some((transform, matrix_at as u64))
    };
    match scope.kind.as_str() {
        "SpherePrimitive"
            if scope.frame_length == 462
                && bytes.get(start + 29) == Some(&1)
                && bytes.get(start + 30) == Some(&1)
                && bytes.get(start + 41) == Some(&1)
                && bytes.get(start + 52) == Some(&1) =>
        {
            let diameter_record_index = u32_at(bytes, start + 42)?;
            let (diameter, diameter_offset) =
                exact_primitive_diameter(bytes, diameter_record_index)?;
            let (transform, transform_offset) = matrix(64)?;
            Some(DesignSolidPrimitive::Sphere {
                transform,
                transform_offset,
                diameter,
                diameter_record_index,
                diameter_offset,
                operation,
                operation_offset: operation_offset as u64,
            })
        }
        "TorusPrimitive"
            if scope.frame_length == 486
                && bytes.get(start + 29) == Some(&1)
                && bytes.get(start + 30) == Some(&1)
                && bytes.get(start + 41) == Some(&1)
                && bytes.get(start + 52) == Some(&1)
                && bytes.get(start + 63) == Some(&1) =>
        {
            let major_diameter_record_index = u32_at(bytes, start + 31)?;
            let minor_diameter_record_index = u32_at(bytes, start + 53)?;
            if major_diameter_record_index == minor_diameter_record_index {
                return None;
            }
            let (major_diameter, major_diameter_offset) =
                exact_primitive_diameter(bytes, major_diameter_record_index)?;
            let (minor_diameter, minor_diameter_offset) =
                exact_primitive_diameter(bytes, minor_diameter_record_index)?;
            let (transform, transform_offset) = matrix(75)?;
            Some(DesignSolidPrimitive::Torus {
                transform,
                transform_offset,
                major_diameter,
                major_diameter_record_index,
                major_diameter_offset,
                minor_diameter,
                minor_diameter_record_index,
                minor_diameter_offset,
                operation,
                operation_offset: operation_offset as u64,
            })
        }
        _ => None,
    }
}

fn exact_primitive_diameter(bytes: &[u8], record_index: u32) -> Option<(f64, u64)> {
    let scalar = exact_fixed_scalar(bytes, record_index)?;
    (scalar.value > 0.0).then_some((scalar.value, scalar.value_offset))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FixedScalarFrame {
    owner_record_index: Option<u32>,
    ordinal: u8,
    value: f64,
    value_offset: u64,
}

fn exact_fixed_scalar(bytes: &[u8], record_index: u32) -> Option<FixedScalarFrame> {
    let mut headers = Vec::new();
    let mut position = 0;
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if u32_at(bytes, at + 7) == Some(record_index) {
            headers.push(at);
        }
        position = at + 1;
    }
    let candidates = headers
        .windows(2)
        .filter_map(|pair| {
            let start = pair[0];
            let frame_length = pair[1].checked_sub(start)?;
            matches!(frame_length, 100 | 103 | 104 | 105).then_some(())?;
            if frame_length == 100 || frame_length == 103 {
                let (class_tag, after_tag) =
                    lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
                if after_tag != start + 7
                    || class_tag.len() != 3
                    || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                    || bytes.get(start + 11..start + 19) != Some(&[0; 8])
                    || bytes.get(start + 19..start + 24) != Some(&[1, 1, 0, 0, 0])
                    || bytes.get(start + 29..start + 35) != Some(&[0; 6])
                    || bytes.get(start + 36..start + 40) != Some(&[0; 4])
                {
                    return None;
                }
                if frame_length == 103
                    && (class_tag != "354"
                        || marked_record_reference(bytes, start + 24).is_none()
                        || marked_record_reference(bytes, start + 48).is_none()
                        || marked_record_reference(bytes, start + 67) != u32_at(bytes, start + 25)
                        || bytes.get(start + 78..start + 80) != Some(&[0; 2])
                        || marked_record_reference(bytes, start + 80).is_none()
                        || bytes.get(start + 85..start + 92) != Some(&[0; 7])
                        || marked_record_reference(bytes, start + 92) != u32_at(bytes, start + 25))
                {
                    return None;
                }
            }
            let value = f64_at(bytes, start + 40)?;
            value.is_finite().then_some(FixedScalarFrame {
                owner_record_index: (bytes.get(start + 24) == Some(&1))
                    .then(|| u32_at(bytes, start + 25))
                    .flatten(),
                ordinal: *bytes.get(start + 35)?,
                value,
                value_offset: u64::try_from(start + 40).ok()?,
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn exact_direct_face_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    match design_feature_family(&scope.kind)? {
        DesignFeatureFamily::OffsetFaces
            if matches!(
                (
                    parameter_scope_payload_length(scope),
                    scope.reference_members.len()
                ),
                (Some(264), 4) | (Some(253), 3)
            ) && bytes.get(start + 25) == Some(&1) =>
        {
            let distance_record_index = u32_at(bytes, start + 26)?;
            if scope.reference_members.last() != Some(&distance_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, distance_record_index)?;
            Some(DesignDirectFaceOperation::OffsetFaces {
                distance: scalar.value,
                distance_record_index,
                distance_offset: scalar.value_offset,
            })
        }
        DesignFeatureFamily::Thicken
            if parameter_scope_payload_length(scope) == Some(287)
                && bytes.get(start + 47) == Some(&1)
                && scope.reference_members.len() == 3 =>
        {
            let thickness_record_index = u32_at(bytes, start + 48)?;
            if scope.reference_members.last() != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, thickness_record_index)?;
            if scalar.value == 0.0 {
                return None;
            }
            Some(DesignDirectFaceOperation::Thicken {
                signed_thickness: scalar.value,
                thickness_record_index,
                thickness_offset: scalar.value_offset,
            })
        }
        DesignFeatureFamily::Shell if scope.reference_members.len() == 3 => {
            let (thickness_record_index, outward, outward_offset) =
                match parameter_scope_payload_length(scope) {
                    Some(268)
                        if matches!(bytes.get(start + 25), Some(0 | 1))
                            && bytes.get(start + 26) == Some(&0)
                            && bytes.get(start + 27) == Some(&1)
                            && u32_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && u32_at(bytes, start + 56)
                                == scope.reference_members.first().copied() =>
                    {
                        (
                            u32_at(bytes, start + 28)?,
                            bytes[start + 25] != 0,
                            start + 25,
                        )
                    }
                    Some(258)
                        if bytes.get(start + 11..start + 21) == Some(&[0; 10])
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22) == Some(&1)
                            && bytes.get(start + 27..start + 42) == Some(&[0; 15])
                            && u32_at(bytes, start + 42) == Some(1)
                            && bytes.get(start + 46) == Some(&1)
                            && u32_at(bytes, start + 47)
                                == scope.reference_members.first().copied()
                            && bytes.get(start + 51..start + 57) == Some(&[0; 6]) =>
                    {
                        (
                            u32_at(bytes, start + 23)?,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    _ => return None,
                };
            if scope.reference_members.last() != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, thickness_record_index)?;
            if scalar.value <= 0.0 {
                return None;
            }
            Some(DesignDirectFaceOperation::Shell {
                thickness: scalar.value,
                thickness_record_index,
                thickness_offset: scalar.value_offset,
                outward,
                outward_offset: u64::try_from(outward_offset).ok()?,
            })
        }
        _ => None,
    }
}

pub(crate) fn exact_move_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignMoveOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Move) {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in indexed_record_pairs(bytes, *record_index) {
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            let frame_length = paired.checked_sub(start)?;
            if u32_at(bytes, after_tag) != Some(*record_index)
                || bytes.get(start + 11..start + 43) != Some(&[0; 32])
            {
                continue;
            }
            if !matches!(
                (class_tag.as_str(), frame_length),
                ("296", 253) | ("349", 254 | 274) | ("368", 254)
            ) || bytes.get(start + 47) != Some(&0)
            {
                continue;
            }
            let form = u32_at(bytes, start + 43)?;
            if !matches!(form, 1 | 5) {
                continue;
            }
            let transform: [[f64; 4]; 4] = f64s_at(bytes, start + 48, 16)?
                .chunks_exact(4)
                .map(|row| row.try_into().expect("four-value matrix row"))
                .collect::<Vec<[f64; 4]>>()
                .try_into()
                .ok()?;
            if !valid_sketch_transform(&transform) {
                continue;
            }
            candidates.push(DesignMoveOperation {
                transform,
                transform_offset: (start + 48) as u64,
                transform_record_index: *record_index,
                form,
                form_offset: (start + 43) as u64,
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(crate) fn exact_scale_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignScaleOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Scale)
        || parameter_scope_payload_length(scope) != Some(303)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let [center_record_index, body_group_record_index, ..] = scope.reference_members.as_slice()
    else {
        return None;
    };
    if u32_at(bytes, start + 20)? != 1
        || bytes.get(start + 24) != Some(&0)
        || marked_record_reference(bytes, start + 33)? != *scope.reference_members.last()?
        || marked_record_reference(bytes, start + 44)? != *center_record_index
        || u32_at(bytes, start + 55)? != 1
        || bytes.get(start + 59) != Some(&0)
        || u32_at(bytes, start + 60)? != 1
        || u32_at(bytes, start + 64)? != 1
        || marked_record_reference(bytes, start + 68)? != *body_group_record_index
    {
        return None;
    }
    let uniform_factor_offset = start + 25;
    let uniform_factor = f64_at(bytes, uniform_factor_offset)?;
    if !uniform_factor.is_finite() || uniform_factor <= 0.0 {
        return None;
    }
    Some(DesignScaleOperation {
        body_group_record_index: *body_group_record_index,
        center_record_index: *center_record_index,
        uniform_factor,
        uniform_factor_offset: uniform_factor_offset as u64,
    })
}

pub(crate) fn exact_fixed_extrude_parameters(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignFixedExtrudeParameters> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Extrude)
        || scope.extrude_extent != Some(DesignExtrudeExtent::OneSidedDistance)
    {
        return None;
    }
    let lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    let [(along_distance_record_index, along), (taper_angle_record_index, taper)] =
        lanes.as_slice()
    else {
        return None;
    };
    if along.ordinal != 0 || taper.ordinal != 1 || along.value == 0.0 {
        return None;
    }
    Some(DesignFixedExtrudeParameters {
        along_distance: along.value,
        along_distance_record_index: *along_distance_record_index,
        along_distance_offset: along.value_offset,
        taper_angle: taper.value,
        taper_angle_record_index: *taper_angle_record_index,
        taper_angle_offset: taper.value_offset,
    })
}

pub(crate) fn exact_fixed_fillet_parameters(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignFixedFilletParameters> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Fillet) {
        return None;
    }
    let lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    if lanes.is_empty()
        || lanes
            .iter()
            .enumerate()
            .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
    {
        return None;
    }
    let (tangency_weight, radius_lanes) = if lanes.len() == 1 {
        (None, lanes.iter().collect::<Vec<_>>())
    } else {
        let (tangency_weight_record_index, tangency) = lanes[0];
        if tangency.value <= 0.0 {
            return None;
        }
        let radius_lanes = if lanes.len() <= 3 {
            lanes[1..].iter().collect::<Vec<_>>()
        } else {
            if lanes.len() % 2 == 0 {
                return None;
            }
            lanes[1..3]
                .iter()
                .chain(lanes[3..].chunks_exact(2).map(|pair| &pair[0]))
                .collect::<Vec<_>>()
        };
        (
            Some(crate::records::DesignFixedFilletTangencyWeight {
                value: tangency.value,
                record_index: tangency_weight_record_index,
                value_offset: tangency.value_offset,
            }),
            radius_lanes,
        )
    };
    let parameter_lanes = if tangency_weight.is_some() {
        lanes
            .get(3..)
            .into_iter()
            .flat_map(|lanes| lanes.chunks_exact(2).map(|pair| &pair[1]))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let radii = radius_lanes
        .iter()
        .map(|(_, scalar)| scalar.value)
        .collect::<Vec<_>>();
    let intermediate_parameters = parameter_lanes
        .iter()
        .map(|(_, scalar)| scalar.value)
        .collect::<Vec<_>>();
    if radii.iter().any(|radius| *radius < 0.0)
        || radii.iter().all(|radius| *radius == 0.0)
        || intermediate_parameters
            .iter()
            .any(|parameter| !(0.0..1.0).contains(parameter))
        || intermediate_parameters
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    Some(DesignFixedFilletParameters {
        tangency_weight,
        radii,
        radius_record_indexes: radius_lanes
            .iter()
            .map(|(record_index, _)| *record_index)
            .collect(),
        radius_offsets: radius_lanes
            .iter()
            .map(|(_, scalar)| scalar.value_offset)
            .collect(),
        intermediate_parameters,
        intermediate_parameter_record_indexes: parameter_lanes
            .iter()
            .map(|(record_index, _)| *record_index)
            .collect(),
        intermediate_parameter_offsets: parameter_lanes
            .iter()
            .map(|(_, scalar)| scalar.value_offset)
            .collect(),
    })
}

pub(crate) fn exact_fixed_chamfer_parameters(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignFixedChamferParameters> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Chamfer) {
        return None;
    }
    let lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    let [(distance_record_index, distance)] = lanes.as_slice() else {
        return None;
    };
    if distance.ordinal != 0 || distance.value <= 0.0 {
        return None;
    }
    Some(DesignFixedChamferParameters {
        distance: distance.value,
        distance_record_index: *distance_record_index,
        distance_offset: distance.value_offset,
    })
}

pub(crate) fn exact_path_feature_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignPathFeatureConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let operation = |offset| {
        Some(match u32_at(bytes, offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        })
    };
    match design_feature_family(&scope.kind)? {
        DesignFeatureFamily::Revolve
            if parameter_scope_payload_length(scope) == Some(372)
                && scope.reference_members.len() == 7
                && u32_at(bytes, start + 29) == Some(2)
                && bytes.get(start + 33) == Some(&0) =>
        {
            let lanes = scope
                .reference_members
                .iter()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index))
                        .then_some((*record_index, scalar))
                })
                .collect::<Vec<_>>();
            let [(angle_record_index, angle), (opposite_angle_record_index, opposite)] =
                lanes.as_slice()
            else {
                return None;
            };
            if angle.ordinal != 0
                || opposite.ordinal != 1
                || angle.value <= 0.0
                || opposite.value != 0.0
            {
                return None;
            }
            Some(DesignPathFeatureConstruction::Revolve {
                operation: operation(start + 25)?,
                operation_offset: u64::try_from(start + 25).ok()?,
                angle: angle.value,
                angle_record_index: *angle_record_index,
                angle_offset: angle.value_offset,
                opposite_angle_record_index: *opposite_angle_record_index,
                opposite_angle_offset: opposite.value_offset,
            })
        }
        DesignFeatureFamily::Loft
            if scope.class_tag.len() == 3
                && parameter_scope_payload_length(scope).is_some_and(|length| length >= 368) =>
        {
            Some(DesignPathFeatureConstruction::Loft {
                operation: operation(start + 29)?,
                operation_offset: u64::try_from(start + 29).ok()?,
            })
        }
        DesignFeatureFamily::Sweep => {
            let lanes = scope
                .reference_members
                .iter()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index))
                        .then_some((*record_index, scalar))
                })
                .collect::<Vec<_>>();
            let lanes: [(u32, FixedScalarFrame); 6] = lanes.try_into().ok()?;
            if lanes
                .iter()
                .enumerate()
                .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
            {
                return None;
            }
            Some(DesignPathFeatureConstruction::Sweep {
                operation: operation(start + 25)?,
                operation_offset: u64::try_from(start + 25).ok()?,
                values: lanes.map(|(_, scalar)| scalar.value),
                record_indexes: lanes.map(|(record_index, _)| record_index),
                value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
            })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScopePlacementFrame {
    pub(crate) transform: [[f64; 4]; 4],
    pub(crate) transform_offset: u64,
    pub(crate) reference: Option<(u32, u64)>,
}

pub(crate) fn exact_work_plane_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in indexed_record_pairs(bytes, *record_index) {
            let frame_length = paired.checked_sub(start)?;
            let (matrix_at, reference) = match frame_length {
                321 if bytes.get(start + 11..start + 49) == Some(&[0u8; 38][..]) => {
                    (start + 49, None)
                }
                326 if bytes.get(start + 11..start + 50) == Some(&[0u8; 39][..]) => {
                    (start + 50, None)
                }
                352 if bytes.get(start + 55) == Some(&1)
                    && bytes.get(start + 56..start + 66) == Some(&[0u8; 10][..]) =>
                {
                    (start + 66, None)
                }
                362 | 373
                    if bytes.get(start + 55..start + 58) == Some(&[1, 0, 1][..])
                        && bytes.get(start + 62..start + 76) == Some(&[0u8; 14][..]) =>
                {
                    (
                        start + 76,
                        Some((u32_at(bytes, start + 58)?, (start + 58) as u64)),
                    )
                }
                _ => continue,
            };
            let values = f64s_at(bytes, matrix_at, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            if !valid_sketch_transform(&transform) {
                continue;
            }
            candidates.push(ScopePlacementFrame {
                transform,
                transform_offset: matrix_at as u64,
                reference,
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn exact_joint_origin_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    if scope.kind != "JointOrigin" {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in indexed_record_pairs(bytes, *record_index) {
            if !matches!(paired.checked_sub(start)?, 336 | 347)
                || bytes.get(start + 11..start + 45)? != [0; 34]
                || bytes.get(start + 50..start + 60)? != [0; 10]
            {
                continue;
            }
            let reference = marked_record_reference(bytes, start + 45)?;
            let values = f64s_at(bytes, start + 60, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            if !valid_sketch_transform(&transform) {
                continue;
            }
            candidates.push(ScopePlacementFrame {
                transform,
                transform_offset: (start + 60) as u64,
                reference: Some((reference, (start + 46) as u64)),
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn exact_work_point_position(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<([f64; 3], u64)> {
    if scope.kind != "WorkPoint" {
        return None;
    }
    let references = scope
        .reference_members
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in indexed_record_pairs(bytes, *record_index) {
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            if class_tag != "282" || u32_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            let frame_length = paired.checked_sub(start)?;
            let position_at = match frame_length {
                197 if bytes.get(start + 15..start + 42) == Some(&[0; 27])
                    && u32_at(bytes, start + 66) == Some(1)
                    && f64s_at(bytes, start + 70, 3) == Some(vec![-1.0; 3])
                    && u32_at(bytes, start + 94) == Some(1) =>
                {
                    start + 42
                }
                208 if bytes.get(start + 15..start + 42) == Some(&[0; 27])
                    && u32_at(bytes, start + 66) == Some(7)
                    && f64s_at(bytes, start + 70, 3) == Some(vec![-1.0; 3])
                    && u32_at(bytes, start + 94) == Some(2) =>
                {
                    start + 42
                }
                207 if bytes.get(start + 15..start + 41) == Some(&[0; 26])
                    && bytes.get(start + 41) == Some(&1)
                    && references.contains(&u32_at(bytes, start + 42)?)
                    && bytes.get(start + 46..start + 52) == Some(&[0; 6])
                    && u32_at(bytes, start + 76) == Some(20)
                    && f64s_at(bytes, start + 80, 3) == Some(vec![-1.0; 3])
                    && u32_at(bytes, start + 104) == Some(1) =>
                {
                    start + 52
                }
                _ => continue,
            };
            let position: [f64; 3] = f64s_at(bytes, position_at, 3)?.try_into().ok()?;
            if position.iter().all(|value| value.is_finite()) {
                candidates.push((position, position_at as u64));
            }
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

fn indexed_record_pairs(bytes: &[u8], record_index: u32) -> Vec<(usize, usize)> {
    let mut headers = Vec::new();
    let mut position = 0;
    while let Some(at) = next_indexed_record_offset_with_index(bytes, position, record_index) {
        headers.push(at);
        position = at + 1;
    }
    headers.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

pub(crate) fn parameter_scope_candidate_headers(bytes: &[u8]) -> Vec<DesignRecordHeader> {
    let mut indexed = HashMap::<u32, Vec<(usize, String)>>::new();
    let mut position = 0;
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if let Some((class_tag, after_tag)) =
            lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)
        {
            if let Some(record_index) = u32_at(bytes, after_tag) {
                indexed
                    .entry(record_index)
                    .or_default()
                    .push((at, class_tag));
            }
        }
        position = at.saturating_add(1);
    }
    indexed
        .into_iter()
        .flat_map(|(record_index, occurrences)| {
            let candidate_count = occurrences.len().saturating_sub(1);
            occurrences
                .into_iter()
                .take(candidate_count)
                .map(move |(at, class_tag)| DesignRecordHeader {
                    id: String::new(),
                    record_index,
                    class_tag,
                    byte_offset: at as u64,
                })
        })
        .collect()
}

pub(crate) fn parse_parameter_scope(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<DesignParameterScope> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut position = start.checked_add(11)?;
    let (paired_at, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (class_tag, after_tag) = lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after_tag)? == header.record_index {
            break (at, class_tag);
        }
        position = at.checked_add(1)?;
    };
    let mut candidates = Vec::new();
    for at in start + 11..paired_at {
        if let Some((kind, end)) = lp_utf16_bounded(bytes, at, 1..=256) {
            let Some(tail_length) = paired_at.checked_sub(end) else {
                continue;
            };
            if (matches!(tail_length, 77 | 78) || (tail_length == 110 && kind == "CopyPasteBodies"))
                && kind.chars().all(|character| !character.is_control())
            {
                candidates.push((at, end, kind));
            }
        }
    }
    let [(kind_at, kind_end, kind)] = candidates.as_slice() else {
        return None;
    };
    let kind_end = *kind_end;
    let reference_table_end = kind_at.checked_sub(4)?;
    let feature_ordinal = u32_at(bytes, kind_end)?;
    if feature_ordinal == 0 {
        return None;
    }
    let history_state_id_offset = reference_table_end;
    let history_state_id = match u32_at(bytes, history_state_id_offset)? {
        u32::MAX => None,
        state_id => Some(i64::from(state_id)),
    };
    let previous_history_state_id_offset =
        kind_end.checked_add(if kind == "CopyPasteBodies" { 53 } else { 31 })?;
    let previous_history_state_id = match u32_at(bytes, previous_history_state_id_offset)? {
        u32::MAX => None,
        state_id => Some(i64::from(state_id)),
    };
    let mut reference_tables = Vec::new();
    for count_at in start + 11..reference_table_end {
        let count = usize::try_from(u32_at(bytes, count_at)?).ok()?;
        if count == 0
            || count_at
                .checked_add(4)?
                .checked_add(count.checked_mul(11)?)?
                != reference_table_end
        {
            continue;
        }
        let first = count_at.checked_add(4)?;
        let mut members = Vec::with_capacity(count);
        let mut offsets = Vec::with_capacity(count);
        for ordinal in 0..count {
            let marker = first.checked_add(ordinal.checked_mul(11)?)?;
            if bytes.get(marker) != Some(&1) || bytes.get(marker + 5..marker + 11)? != [0; 6] {
                members.clear();
                break;
            }
            members.push(u32_at(bytes, marker + 1)?);
            offsets.push(u64::try_from(marker + 1).ok()?);
        }
        if members.len() == count {
            reference_tables.push((count_at, members, offsets));
        }
    }
    let [(reference_count_at, reference_members, reference_member_offsets)] =
        reference_tables.as_slice()
    else {
        return None;
    };
    let surface_stitch_operation = if kind == "SurfaceStitch" {
        exact_surface_stitch_operation(bytes, header.record_index, reference_members)
    } else {
        None
    };
    let base_flange_operation = if kind == "BaseFlange" {
        exact_base_flange_operation(bytes, start, paired_at, reference_members)
    } else {
        None
    };
    let edge_flange_operation = if kind == "EdgeFlange" {
        exact_edge_flange_operation(bytes, start, paired_at, reference_members)
    } else {
        None
    };
    let hem_operation = if kind == "Hem" {
        exact_hem_operation(bytes, start, paired_at, reference_members)
    } else {
        None
    };
    let family = design_feature_family(kind);
    // A `Sketch` scope carries either the single entity-suffix reference form
    // or, when the stream's sketch entity headers use the `EntityGenesis`
    // form, the generic ordered reference table. Both parse here; the entity
    // binding in `decode_parameter_scopes` requires a unique suffix match.
    let (
        extrude_operation,
        extrude_operation_offset,
        extrude_extent,
        extrude_extent_offsets,
        extrude_direction_reversed,
        extrude_direction_reversed_offset,
        extrude_start,
        extrude_start_offset,
    ) = if family == Some(DesignFeatureFamily::Extrude) {
        let direct_offset = start.checked_add(28)?;
        let referenced_offset = start.checked_add(38)?;
        let operation_offset = if bytes.get(start.checked_add(25)?) == Some(&1)
            && bytes.get(start.checked_add(30)?..start.checked_add(36)?)? == [0; 6]
        {
            referenced_offset
        } else {
            direct_offset
        };
        let operation = match u32_at(bytes, operation_offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        };
        let side_offset = operation_offset.checked_add(4)?;
        let termination_offset = operation_offset.checked_add(8)?;
        let extent = match (
            u32_at(bytes, side_offset)?,
            u32_at(bytes, termination_offset)?,
        ) {
            (1, 1) => DesignExtrudeExtent::OneSidedToFace,
            (1, 2) => DesignExtrudeExtent::OneSidedDistance,
            (2, 0) => DesignExtrudeExtent::TwoSidedDistance,
            _ => return None,
        };
        let direction_reversed_offset = operation_offset.checked_add(12)?;
        let direction_reversed = match bytes.get(direction_reversed_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        if bytes.get(operation_offset.checked_add(13)?)? != &1 {
            return None;
        }
        let start_offset = operation_offset.checked_add(14)?;
        let start = match bytes.get(start_offset)? {
            0 => DesignExtrudeStart::ProfilePlane,
            1 => DesignExtrudeStart::OffsetProfilePlane,
            2 => DesignExtrudeStart::FromFace,
            _ => return None,
        };
        (
            Some(operation),
            Some(operation_offset as u64),
            Some(extent),
            Some([side_offset as u64, termination_offset as u64]),
            Some(direction_reversed),
            Some(direction_reversed_offset as u64),
            Some(start),
            Some(start_offset as u64),
        )
    } else {
        (None, None, None, None, None, None, None, None)
    };
    let (
        coil_operation,
        coil_operation_offset,
        coil_extent,
        coil_extent_offset,
        coil_section,
        coil_section_offset,
        coil_section_placement,
        coil_section_placement_offset,
        coil_clockwise,
        coil_clockwise_offset,
    ) = if family == Some(DesignFeatureFamily::Coil) {
        let operation_offset = start.checked_add(20)?;
        let operation = match u32_at(bytes, operation_offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        };
        let clockwise_offset = start.checked_add(24)?;
        let clockwise = match bytes.get(clockwise_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        if u32_at(bytes, start.checked_add(26)?)? != 2 {
            return None;
        }
        let extent_offset = start.checked_add(30)?;
        let extent = match u32_at(bytes, extent_offset)? {
            1 => DesignCoilExtent::RevolutionsHeight,
            2 => DesignCoilExtent::RevolutionsPitch,
            3 => DesignCoilExtent::HeightPitch,
            4 => DesignCoilExtent::Spiral,
            _ => return None,
        };
        let section_offset = start.checked_add(92)?;
        let section = match u32_at(bytes, section_offset)? {
            0 => DesignCoilSection::Circular,
            1 => DesignCoilSection::Square,
            2 => DesignCoilSection::ExternalTriangle,
            3 => DesignCoilSection::InternalTriangle,
            _ => return None,
        };
        let section_placement_offset = start.checked_add(107)?;
        let section_placement = match u32_at(bytes, section_placement_offset)? {
            4 => DesignCoilSectionPlacement::Inside,
            _ => return None,
        };
        (
            Some(operation),
            Some(operation_offset as u64),
            Some(extent),
            Some(extent_offset as u64),
            Some(section),
            Some(section_offset as u64),
            Some(section_placement),
            Some(section_placement_offset as u64),
            Some(clockwise),
            Some(clockwise_offset as u64),
        )
    } else {
        (None, None, None, None, None, None, None, None, None, None)
    };
    Some(DesignParameterScope {
        id: String::new(),
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        record_index: header.record_index,
        frame_length: u64::try_from(paired_at.checked_sub(start)?).ok()?,
        kind: kind.clone(),
        kind_offset: u64::try_from(kind_at.checked_add(4)?).ok()?,
        extrude_operation,
        extrude_operation_offset,
        extrude_extent,
        extrude_extent_offsets,
        extrude_direction_reversed,
        extrude_direction_reversed_offset,
        extrude_start,
        extrude_start_offset,
        coil_operation,
        coil_operation_offset,
        coil_extent,
        coil_extent_offset,
        coil_section,
        coil_section_offset,
        coil_section_placement,
        coil_section_placement_offset,
        coil_clockwise,
        coil_clockwise_offset,
        feature_ordinal,
        feature_ordinal_offset: u64::try_from(kind_end).ok()?,
        history_state_id,
        history_state_id_offset: u64::try_from(history_state_id_offset).ok()?,
        previous_history_state_id,
        previous_history_state_id_offset: u64::try_from(previous_history_state_id_offset).ok()?,
        reference_count_offset: u64::try_from(*reference_count_at).ok()?,
        reference_members: reference_members.clone(),
        reference_member_offsets: reference_member_offsets.clone(),
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation,
        base_flange_operation,
        edge_flange_operation,
        hem_operation,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag,
        paired_byte_offset: paired_at as u64,
    })
}

pub(crate) fn exact_surface_stitch_operation(
    bytes: &[u8],
    scope_record_index: u32,
    references: &[u32],
) -> Option<DesignSurfaceStitchOperation> {
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return None;
    }
    let tolerance_record_index = references[references.len() - 2];
    let settings_record_index = references[references.len() - 1];
    let scalar = exact_fixed_scalar(bytes, tolerance_record_index)?;
    if scalar.owner_record_index != Some(scope_record_index) || scalar.ordinal != 0 {
        return None;
    }
    let gap_tolerance = scalar.value;
    if !gap_tolerance.is_finite() || gap_tolerance <= 0.0 {
        return None;
    }
    Some(DesignSurfaceStitchOperation {
        gap_tolerance,
        gap_tolerance_offset: scalar.value_offset,
        tolerance_record_index,
        settings_record_index,
    })
}

pub(crate) fn exact_base_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignBaseFlangeOperation> {
    let [profile_group_record_index, profile_record_index, thickness_record_index, settings_record_index] =
        references
    else {
        return None;
    };
    if paired_at.checked_sub(start)? != 416
        || u32_at(bytes, start + 73)? != 1
        || bytes.get(start + 81) != Some(&1)
        || u32_at(bytes, start + 82)? != *settings_record_index
        || bytes.get(start + 86..start + 92)? != [0; 6]
        || u32_at(bytes, start + 92)? != 1
        || bytes.get(start + 112) != Some(&1)
        || u32_at(bytes, start + 113)? != *thickness_record_index
        || bytes.get(start + 117..start + 123)? != [0; 6]
        || u32_at(bytes, start + 141)? != 1
        || bytes.get(start + 145) != Some(&1)
        || u32_at(bytes, start + 146)? != *profile_group_record_index
        || bytes.get(start + 150..start + 156)? != [0; 6]
    {
        return None;
    }
    let thickness = f64_at(bytes, start + 123)?;
    if !thickness.is_finite() || thickness <= 0.0 {
        return None;
    }
    Some(DesignBaseFlangeOperation {
        thickness,
        thickness_offset: u64::try_from(start + 123).ok()?,
        profile_group_record_index: *profile_group_record_index,
        profile_record_index: *profile_record_index,
        thickness_record_index: *thickness_record_index,
        settings_record_index: *settings_record_index,
    })
}

pub(crate) fn exact_edge_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignEdgeFlangeOperation> {
    let edge_count = usize::try_from(u32_at(bytes, start.checked_add(30)?)?).ok()?;
    if edge_count == 0 || references.len() != edge_count.checked_mul(4)?.checked_add(4)? {
        return None;
    }
    let height_owner_record_index = references[edge_count * 3];
    let angle_owner_record_index = references[edge_count * 3 + 1];
    let aggregate_group_record_index = references[edge_count * 3 + 2];
    let aggregate_operand_record_indices = references
        .get(edge_count * 3 + 3..edge_count * 4 + 3)?
        .to_vec();
    let settings_record_index = *references.last()?;
    let common = start
        .checked_add(69)?
        .checked_add(edge_count.checked_mul(16)?)?;
    let extent_code = u32_at(bytes, common)?;
    if usize::try_from(u32_at(bytes, common + 4)?).ok()? != edge_count {
        return None;
    }
    let mut edge_wrapper_record_indices = Vec::with_capacity(edge_count);
    let mut edge_group_record_indices = Vec::with_capacity(edge_count);
    let mut edge_operand_record_indices = Vec::with_capacity(edge_count);
    let mut cursor = common.checked_add(8)?;
    for ordinal in 0..edge_count {
        let wrapper = marked_record_reference(bytes, cursor)?;
        if wrapper != references[ordinal * 3] {
            return None;
        }
        edge_wrapper_record_indices.push(wrapper);
        edge_group_record_indices.push(references[ordinal * 3 + 1]);
        edge_operand_record_indices.push(references[ordinal * 3 + 2]);
        cursor = cursor.checked_add(11)?;
    }
    if marked_record_reference(bytes, cursor)? != settings_record_index {
        return None;
    }
    cursor = cursor.checked_add(11)?;
    let height_datum_code = u32_at(bytes, cursor)?;
    cursor = cursor.checked_add(4)?;
    if marked_record_reference(bytes, cursor)? != angle_owner_record_index {
        return None;
    }
    cursor = cursor.checked_add(11)?;
    if marked_record_reference(bytes, cursor)? != height_owner_record_index {
        return None;
    }
    cursor = cursor.checked_add(11)?;
    let bend_position_code = u32_at(bytes, cursor)?;
    let bend_radius_offset = cursor.checked_add(15)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let result_count = usize::try_from(u32_at(bytes, bend_radius_offset.checked_add(14)?)?).ok()?;
    let expected_length = 411usize
        .checked_add(edge_count.checked_mul(82)?)?
        .checked_add(result_count.checked_mul(15)?)?;
    if paired_at.checked_sub(start)? != expected_length {
        return None;
    }
    Some(DesignEdgeFlangeOperation {
        edge_wrapper_record_indices,
        edge_group_record_indices,
        edge_operand_record_indices,
        aggregate_group_record_index,
        aggregate_operand_record_indices,
        height_owner_record_index,
        angle_owner_record_index,
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        extent_code,
        height_datum_code,
        bend_position_code,
    })
}

pub(crate) fn exact_hem_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignHemOperation> {
    let [gap_owner_record_index, length_owner_record_index, edge_wrapper_record_index, edge_group_record_index, edge_operand_record_index, aggregate_group_record_index, aggregate_operand_record_index, settings_record_index] =
        references
    else {
        return None;
    };
    if paired_at.checked_sub(start)? != 494
        || u32_at(bytes, start + 89)? != 1
        || marked_record_reference(bytes, start + 93)? != *edge_wrapper_record_index
        || marked_record_reference(bytes, start + 104)? != *settings_record_index
        || marked_record_reference(bytes, start + 127)? != *gap_owner_record_index
        || marked_record_reference(bytes, start + 138)? != *length_owner_record_index
        || !matches!(bytes.get(start + 119), Some(0 | 1))
    {
        return None;
    }
    let bend_radius_offset = start.checked_add(156)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    Some(DesignHemOperation {
        edge_wrapper_record_index: *edge_wrapper_record_index,
        edge_group_record_index: *edge_group_record_index,
        edge_operand_record_index: *edge_operand_record_index,
        aggregate_group_record_index: *aggregate_group_record_index,
        aggregate_operand_record_index: *aggregate_operand_record_index,
        gap_owner_record_index: *gap_owner_record_index,
        length_owner_record_index: *length_owner_record_index,
        settings_record_index: *settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        form_code: u32_at(bytes, start + 85)?,
        direction_code: u32_at(bytes, start + 115)?,
        is_flipped: bytes[start + 119] != 0,
        bend_position_code: u32_at(bytes, start + 121)?,
    })
}

fn marked_record_reference(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || bytes.get(at + 5..at + 11)? != [0; 6] {
        return None;
    }
    u32_at(bytes, at + 1)
}

fn parameter_scope_payload_length(scope: &DesignParameterScope) -> Option<u64> {
    let kind_bytes = u64::try_from(scope.kind.encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    scope.frame_length.checked_sub(kind_bytes)
}
