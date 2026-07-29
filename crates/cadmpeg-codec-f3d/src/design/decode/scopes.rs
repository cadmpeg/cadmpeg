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
    DesignAssemblyAlignment, DesignAssemblyOperandFrame, DesignAssemblyOperandPath,
    DesignBaseFeatureConstruction, DesignBaseFlangeOperation, DesignCircularPatternConstruction,
    DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement, DesignCombineOperation,
    DesignComponentInsertConstruction, DesignComponentOccurrence,
    DesignComponentPatternOccurrences, DesignCopyPasteBodiesOperation,
    DesignCopyPasteComponentOperation, DesignDirectFaceOperation, DesignDraftOperation,
    DesignEdgeFlangeOperation, DesignEntityHeader, DesignExtrudeExtent, DesignExtrudeOperation,
    DesignExtrudePrologue, DesignExtrudePrologueReference, DesignExtrudeStart,
    DesignFixedChamferDistance, DesignFixedChamferParameters, DesignFixedExtrudeParameters,
    DesignFixedFilletGroup, DesignFixedFilletParameters, DesignHemOperation,
    DesignMirrorConstruction, DesignMoveOperation, DesignObjectKind, DesignParameterOwner,
    DesignParameterScope, DesignPathFeatureConstruction, DesignRecordHeader,
    DesignRectangularPatternConstruction, DesignRectangularPatternInstances, DesignScaleOperation,
    DesignSolidPrimitive, DesignSurfaceStitchOperation,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{f64_at, f64s_at, u32_at, u64_at as read_u64};
use std::collections::{HashMap, HashSet};

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub fn decode_parameter_scopes(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
    parameter_owners: &[crate::records::DesignParameterOwner],
    component_occurrences: &[DesignComponentOccurrence],
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
            scope.id = ids::native_design_parameter_scope_id(&entry.name, scope.byte_offset);
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
            scope.combine_operation = exact_combine_operation(bytes, &scope);
            scope.draft_operation = exact_draft_operation(bytes, &scope);
            scope.circular_pattern_construction =
                exact_circular_pattern_construction_with_owners(bytes, &scope, parameter_owners);
            scope.rectangular_pattern_construction =
                exact_rectangular_pattern_construction(bytes, &scope, parameter_owners);
            scope.assembly_alignment = exact_assembly_alignment(bytes, &scope, parameter_owners);
            scope.component_insert_construction =
                exact_component_insert_construction(bytes, &scope);
            scope.copy_paste_component_operation =
                exact_copy_paste_component_operation(bytes, &scope, component_occurrences);
            bind_component_pattern_occurrences(&mut scope, component_occurrences);
            scope.copy_paste_bodies_operation = exact_copy_paste_bodies_operation(bytes, &scope);
            scope.base_feature_construction = exact_base_feature_construction(bytes, &scope);
            out.push(scope);
        }
    }
    out.sort_by_key(|scope| scope.id.clone());
    out.dedup_by_key(|scope| scope.id.clone());
    Ok(out)
}

pub(crate) fn exact_assembly_alignment(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignAssemblyAlignment> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Assemble) {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut lanes = parameter_owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && owner.evaluated_value.is_finite()
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(|owner| owner.local_ordinal);
    let [angle, offset_x, offset_y, offset_z] = lanes.as_slice() else {
        return None;
    };
    if [angle, offset_x, offset_y, offset_z]
        .iter()
        .enumerate()
        .any(|(ordinal, owner)| owner.local_ordinal != ordinal as u32)
    {
        return None;
    }
    let owner_record_indices = [
        angle.record_index,
        offset_x.record_index,
        offset_y.record_index,
        offset_z.record_index,
    ];
    if !scope.reference_members.ends_with(&owner_record_indices) {
        return None;
    }
    let mut alignment = DesignAssemblyAlignment {
        angle: angle.evaluated_value,
        offset: [
            offset_x.evaluated_value,
            offset_y.evaluated_value,
            offset_z.evaluated_value,
        ],
        owner_record_indices,
        value_offsets: [
            angle.evaluated_value_offset,
            offset_x.evaluated_value_offset,
            offset_y.evaluated_value_offset,
            offset_z.evaluated_value_offset,
        ],
        operand_frames: None,
        operand_paths: None,
    };
    alignment.operand_frames = exact_assembly_operand_frames(bytes, scope);
    alignment.operand_paths = alignment
        .operand_frames
        .as_ref()
        .and_then(|frames| exact_assembly_operand_paths(bytes, scope, frames));
    Some(alignment)
}

pub(crate) fn exact_component_insert_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignComponentInsertConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let relation_record_index = *scope.reference_members.first()?;
    if scope.kind != "Component Insert"
        || scope.frame_length != 399
        || scope.paired_class_tag != "259"
        || scope.reference_members.len() != 1
        || bytes.get(start + 11..start + 20)? != [0; 9]
        || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || bytes.get(start + 33..start + 37)? != [0; 4]
        || bytes.get(start + 37) != Some(&1)
        || u32_at(bytes, start + 38)? != relation_record_index
        || bytes.get(start + 42..start + 50)? != [0, 0, 0, 0, 0, 0, 1, 0]
    {
        return None;
    }
    let transform = rigid_transform_at(bytes, start + 50)?;
    let relation_at = next_indexed_record_offset_with_index(bytes, 0, relation_record_index)?;
    if relation_at >= start
        || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 57
        || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
        || bytes.get(relation_at + 21) != Some(&1)
        || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
        || bytes.get(relation_at + 34) != Some(&1)
        || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
        || bytes.get(relation_at + 46) != Some(&1)
        || u32_at(bytes, relation_at + 47)? != scope.record_index
        || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
    {
        return None;
    }
    let carrier_record_index = u32_at(bytes, relation_at + 22)?;
    let carrier_at = unique_indexed_record_before(bytes, carrier_record_index, relation_at)?;
    let mut placements = Vec::new();
    for at in carrier_at + 11..relation_at {
        let Some((role, after_role)) = lp_utf16_bounded(bytes, at, 1..=256) else {
            continue;
        };
        if !crate::bytes::is_guid_relaxed(&role)
            || bytes.get(after_role..after_role + 2) != Some(&[0, 0])
        {
            continue;
        }
        let transform_at = after_role.checked_add(2)?;
        if rigid_transform_at(bytes, transform_at) == Some(transform) {
            placements.push((role, at + 4, transform_at));
        }
    }
    let [(neutron_role, neutron_role_offset, carrier_transform_offset)] = placements.as_slice()
    else {
        return None;
    };
    Some(DesignComponentInsertConstruction {
        relation_record_index,
        carrier_record_index,
        neutron_role: neutron_role.clone(),
        neutron_role_offset: u64::try_from(*neutron_role_offset).ok()?,
        transform,
        transform_offset: u64::try_from(start + 50).ok()?,
        carrier_transform_offset: u64::try_from(*carrier_transform_offset).ok()?,
    })
}

fn exact_copy_paste_component_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Option<DesignCopyPasteComponentOperation> {
    let stream = native_stream(&scope.id)?;
    let start = usize::try_from(scope.byte_offset).ok()?;
    let relation_record_index = *scope.reference_members.first()?;
    if scope.kind != "CopyPaste"
        || scope.frame_length != 529
        || scope.class_tag != "454"
        || scope.paired_class_tag != "259"
        || scope.reference_members.len() != 1
    {
        return None;
    }
    let source_transform = rigid_transform_at(bytes, start + 38)?;
    let copied_transform = rigid_transform_at(bytes, start + 194)?;
    let relation_at = next_indexed_record_offset_with_index(bytes, 0, relation_record_index)?;
    if relation_at >= start
        || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 57
        || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
        || bytes.get(relation_at + 21) != Some(&1)
        || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
        || bytes.get(relation_at + 34) != Some(&1)
        || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
        || bytes.get(relation_at + 46) != Some(&1)
        || u32_at(bytes, relation_at + 47)? != scope.record_index
        || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
    {
        return None;
    }
    let copied_occurrence_record_index = u32_at(bytes, relation_at + 22)?;
    let copied_candidates = occurrences
        .iter()
        .filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.record_index == copied_occurrence_record_index
                && occurrence.byte_offset < relation_at as u64
                && occurrence.transform == Some(copied_transform)
        })
        .collect::<Vec<_>>();
    let [copied] = copied_candidates.as_slice() else {
        return None;
    };
    let source_candidates = occurrences
        .iter()
        .filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.byte_offset < copied.byte_offset
                && occurrence
                    .component_guid
                    .eq_ignore_ascii_case(&copied.component_guid)
                && occurrence.transform.is_none()
        })
        .collect::<Vec<_>>();
    let [source] = source_candidates.as_slice() else {
        return None;
    };
    Some(DesignCopyPasteComponentOperation {
        relation_record_index,
        source_occurrence_record_index: source.record_index,
        copied_occurrence_record_index,
        component_guid: copied.component_guid.clone(),
        source_occurrence_guid: source.occurrence_guid.clone(),
        copied_occurrence_guid: copied.occurrence_guid.clone(),
        source_transform,
        source_transform_offset: u64::try_from(start + 38).ok()?,
        copied_transform,
        copied_transform_offset: u64::try_from(start + 194).ok()?,
    })
}

fn bind_component_pattern_occurrences(
    scope: &mut DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) {
    let Some(stream) = native_stream(&scope.id) else {
        return;
    };
    let Some(instances) = scope
        .rectangular_pattern_construction
        .as_mut()
        .and_then(|construction| construction.instances.as_mut())
    else {
        return;
    };
    let mut generated = Vec::new();
    for (ordinal, transform_offset) in instances.transform_offsets.iter().enumerate().skip(1) {
        let candidates = occurrences
            .iter()
            .filter(|occurrence| {
                native_stream(&occurrence.id) == Some(stream)
                    && occurrence.transform_offset == Some(*transform_offset)
                    && occurrence.occurrence_ordinal == ordinal as u32 + 1
            })
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return;
        };
        generated.push(*candidate);
    }
    let Some(component_guid) = generated
        .first()
        .map(|occurrence| &occurrence.component_guid)
    else {
        return;
    };
    if generated.iter().any(|occurrence| {
        !occurrence
            .component_guid
            .eq_ignore_ascii_case(component_guid)
    }) {
        return;
    }
    let seed_candidates = occurrences
        .iter()
        .filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.byte_offset < scope.byte_offset
                && occurrence
                    .component_guid
                    .eq_ignore_ascii_case(component_guid)
                && occurrence.occurrence_ordinal == 1
                && occurrence.transform.is_none()
        })
        .collect::<Vec<_>>();
    let [seed] = seed_candidates.as_slice() else {
        return;
    };
    instances.component_occurrences = Some(DesignComponentPatternOccurrences {
        component_guid: component_guid.clone(),
        seed_occurrence_guid: seed.occurrence_guid.clone(),
        generated_occurrence_guids: generated
            .iter()
            .map(|occurrence| occurrence.occurrence_guid.clone())
            .collect(),
    });
}

fn unique_indexed_record_before(bytes: &[u8], record_index: u32, end: usize) -> Option<usize> {
    let mut position = 0;
    let mut found = None;
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if at >= end {
            break;
        }
        let (_, after_tag) = lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after_tag) == Some(record_index) && found.replace(at).is_some() {
            return None;
        }
        position = at.checked_add(1)?;
    }
    found
}

pub(crate) fn rigid_transform_at(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let values = f64s_at(bytes, at, 16)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    valid_sketch_transform(&transform).then_some(transform)
}

fn exact_assembly_operand_frames(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    if !matches!(scope.frame_length, 637 | 692)
        || scope.paired_class_tag != "259"
        || usize::try_from(scope.paired_byte_offset).ok()?
            != start.checked_add(usize::try_from(scope.frame_length).ok()?)?
        || bytes.get(start + 11..start + 20)? != [0; 9]
        || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || !matches!(bytes.get(start + 25), Some(0 | 1))
        || bytes.get(start + 26..start + 28)? != [0; 2]
        || bytes.get(start + 33..start + 40)? != [0; 7]
        || bytes.get(start + 173..start + 180)? != [0; 7]
        || bytes.get(start + 308..start + 312)? != [0; 4]
    {
        return None;
    }
    let frame = |reference_at: usize, transform_at: usize| {
        let reference_record_index = marked_record_reference(bytes, reference_at)?;
        let values = f64s_at(bytes, transform_at, 16)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        if !valid_sketch_transform(&transform) {
            return None;
        }
        Some(DesignAssemblyOperandFrame {
            reference_record_index,
            reference_offset: (reference_at + 1) as u64,
            transform,
            transform_offset: transform_at as u64,
        })
    };
    let first = frame(start + 28, start + 40)?;
    let second = frame(start + 168, start + 180)?;
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

fn exact_assembly_operand_paths(
    bytes: &[u8],
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Option<[DesignAssemblyOperandPath; 2]> {
    let search_start = usize::try_from(scope.paired_byte_offset).ok()?;
    let construction_at = next_indexed_record_offset_with_index(
        bytes,
        search_start,
        frames[0].reference_record_index,
    )?;
    let first_record_index = frames[0].reference_record_index.checked_sub(5)?;
    let second_record_index = frames[0].reference_record_index.checked_sub(2)?;
    let first_at = next_indexed_record_offset_with_index(bytes, search_start, first_record_index)?;
    let second_at =
        next_indexed_record_offset_with_index(bytes, first_at + 11, second_record_index)?;
    if !(first_at < second_at && second_at < construction_at) {
        return None;
    }
    let first = exact_assembly_operand_path(bytes, first_at, first_record_index, second_at)?;
    let second =
        exact_assembly_operand_path(bytes, second_at, second_record_index, construction_at)?;
    Some([first, second])
}

fn exact_assembly_operand_path(
    bytes: &[u8],
    start: usize,
    record_index: u32,
    limit: usize,
) -> Option<DesignAssemblyOperandPath> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 1..=8, u8::is_ascii_digit)?;
    if class_tag != "329"
        || read_u64(bytes, after_tag)? != u64::from(record_index)
        || bytes.get(after_tag + 8..after_tag + 14)? != [0; 6]
    {
        return None;
    }
    let count = usize::try_from(u32_at(bytes, after_tag + 14)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = after_tag + 18;
    let mut occurrence_guids = Vec::with_capacity(count);
    let mut occurrence_guid_offsets = Vec::with_capacity(count);
    for _ in 0..count {
        let (guid, after_guid) = lp_utf16_bounded(bytes.get(..limit)?, position, 36..=36)?;
        if !crate::bytes::is_guid_relaxed(&guid) {
            return None;
        }
        occurrence_guid_offsets.push(u64::try_from(position + 4).ok()?);
        occurrence_guids.push(guid);
        position = after_guid;
    }
    Some(DesignAssemblyOperandPath {
        record_index,
        byte_offset: u64::try_from(start).ok()?,
        occurrence_guids,
        occurrence_guid_offsets,
    })
}

pub(crate) fn exact_rectangular_pattern_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignRectangularPatternConstruction> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::RectangularPattern) {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let mut lanes = parameter_owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && owner.evaluated_value.is_finite()
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(|owner| owner.local_ordinal);
    let [u_count, v_count, u_extent, v_extent] = lanes.as_slice() else {
        return None;
    };
    if [u_count, v_count, u_extent, v_extent]
        .iter()
        .enumerate()
        .any(|(ordinal, owner)| owner.local_ordinal != ordinal as u32)
    {
        return None;
    }
    let exact_count = |value: f64| {
        (value > 0.0 && value <= f64::from(u32::MAX) && value.fract() == 0.0)
            .then_some(value as u32)
    };
    let u_count_value = exact_count(u_count.evaluated_value)?;
    let v_count_value = exact_count(v_count.evaluated_value)?;
    if u_count_value == 1 && v_count_value == 1
        || (u_count_value > 1 && u_extent.evaluated_value == 0.0)
        || (v_count_value > 1 && v_extent.evaluated_value == 0.0)
        || (u_count_value == 1 && u_extent.evaluated_value != 0.0)
        || (v_count_value == 1 && v_extent.evaluated_value != 0.0)
    {
        return None;
    }
    let mut construction = DesignRectangularPatternConstruction {
        u_count: u_count_value,
        v_count: v_count_value,
        u_extent: u_extent.evaluated_value,
        v_extent: v_extent.evaluated_value,
        owner_record_indices: [
            u_count.record_index,
            v_count.record_index,
            u_extent.record_index,
            v_extent.record_index,
        ],
        value_offsets: [
            u_count.evaluated_value_offset,
            v_count.evaluated_value_offset,
            u_extent.evaluated_value_offset,
            v_extent.evaluated_value_offset,
        ],
        instances: None,
    };
    construction.instances = exact_rectangular_pattern_instances(bytes, scope, &construction);
    Some(construction)
}

fn exact_rectangular_pattern_instances(
    bytes: &[u8],
    scope: &DesignParameterScope,
    construction: &DesignRectangularPatternConstruction,
) -> Option<DesignRectangularPatternInstances> {
    let active = [
        (construction.u_count, construction.u_extent),
        (construction.v_count, construction.v_extent),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 1)
    .collect::<Vec<_>>();
    let [(count, extent)] = active.as_slice() else {
        return None;
    };
    let count = usize::try_from(*count).ok()?;
    if count > 4_096
        || scope.reference_members.len() != count.checked_add(6)?
        || scope.reference_members.get(1..5) != Some(&construction.owner_record_indices)
    {
        return None;
    }
    let mut record_indices = Vec::with_capacity(count);
    record_indices.push(*scope.reference_members.first()?);
    record_indices.extend_from_slice(scope.reference_members.get(6..count.checked_add(5)?)?);
    if record_indices.len() != count {
        return None;
    }
    let reference_starts = scope
        .reference_members
        .iter()
        .map(|record_index| {
            next_indexed_record_offset_with_index(bytes, 0, *record_index)
                .map(|offset| (*record_index, offset))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut candidates = Vec::with_capacity(count);
    let mut scanned_bytes = 0_usize;
    for record_index in &record_indices {
        let start = reference_starts
            .iter()
            .find_map(|(candidate, offset)| (candidate == record_index).then_some(*offset))?;
        let end = reference_starts
            .iter()
            .filter_map(|(_, offset)| (*offset > start).then_some(*offset))
            .min()?;
        let span = end.checked_sub(start)?;
        scanned_bytes = scanned_bytes.checked_add(span)?;
        if span > 1_048_576 || scanned_bytes > 16_777_216 {
            return None;
        }
        candidates.push(exact_rigid_transform_candidates(bytes, start, end)?);
    }
    let first_candidates = candidates.first()?;
    let final_candidates = candidates.last()?;
    let mut runs = Vec::new();
    for first in first_candidates {
        for final_candidate in final_candidates {
            if !same_transform_basis(&first.0, &final_candidate.0) {
                continue;
            }
            let delta = translation_delta(&first.0, &final_candidate.0);
            let distance = delta.iter().map(|value| value * value).sum::<f64>().sqrt();
            if (distance - extent.abs()).abs() > 1.0e-8 {
                continue;
            }
            let mut run = vec![*first];
            let mut unique = true;
            for (ordinal, record_candidates) in candidates[1..count - 1].iter().enumerate() {
                let fraction = (ordinal + 1) as f64 / (count - 1) as f64;
                let matches = record_candidates
                    .iter()
                    .filter(|candidate| {
                        same_transform_basis(&first.0, &candidate.0)
                            && translation_delta(&first.0, &candidate.0)
                                .iter()
                                .zip(delta)
                                .all(|(value, total)| (*value - total * fraction).abs() <= 1.0e-8)
                    })
                    .collect::<Vec<_>>();
                let [candidate] = matches.as_slice() else {
                    unique = false;
                    break;
                };
                run.push(**candidate);
            }
            if unique {
                run.push(*final_candidate);
                runs.push(run);
            }
        }
    }
    runs.sort_by_key(|run| run.iter().map(|(_, offset)| *offset).collect::<Vec<_>>());
    runs.dedup_by(|left, right| left == right);
    let [run] = runs.as_slice() else {
        return None;
    };
    Some(DesignRectangularPatternInstances {
        record_indices,
        transforms: run.iter().map(|(transform, _)| *transform).collect(),
        transform_offsets: run.iter().map(|(_, offset)| *offset).collect(),
        component_occurrences: None,
    })
}

type TransformCandidate = ([[f64; 4]; 4], u64);

fn exact_rigid_transform_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Option<Vec<TransformCandidate>> {
    let mut candidates = Vec::new();
    for offset in start..end.checked_sub(127)? {
        let values = f64s_at(bytes, offset, 16)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        if valid_sketch_transform(&transform) {
            candidates.push((transform, u64::try_from(offset).ok()?));
        }
    }
    (!candidates.is_empty()).then_some(candidates)
}

fn same_transform_basis(left: &[[f64; 4]; 4], right: &[[f64; 4]; 4]) -> bool {
    (0..3).all(|row| (0..3).all(|column| (left[row][column] - right[row][column]).abs() <= 1.0e-10))
}

fn translation_delta(left: &[[f64; 4]; 4], right: &[[f64; 4]; 4]) -> [f64; 3] {
    [
        right[0][3] - left[0][3],
        right[1][3] - left[1][3],
        right[2][3] - left[2][3],
    ]
}

pub(crate) fn exact_circular_pattern_construction_with_owners(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[crate::records::DesignParameterOwner],
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
    let owner_count_candidates = parameter_owners.iter().filter_map(|owner| {
        if native_stream(&owner.id) != native_stream(&scope.id)
            || owner.scope_record_index != scope.record_index
            || owner.local_ordinal != 0
            || !owner.evaluated_value.is_finite()
            || owner.evaluated_value <= 0.0
            || owner.evaluated_value > f64::from(u32::MAX)
            || owner.evaluated_value.fract() != 0.0
        {
            return None;
        }
        Some((
            owner.evaluated_value as u32,
            owner.record_index,
            owner.evaluated_value_offset,
        ))
    });
    let mut count_candidates = owner_count_candidates.collect::<Vec<_>>();
    if count_candidates.is_empty() {
        count_candidates.extend(scope.reference_members.iter().filter_map(|record_index| {
            exact_fixed_pattern_count(bytes, *record_index, scope.record_index)
                .map(|(count, count_offset)| (count, *record_index, count_offset))
        }));
    }
    count_candidates.sort_unstable();
    count_candidates.dedup();
    let [(count, count_record_index, count_offset)] = count_candidates.as_slice() else {
        return None;
    };
    let owner_angle_candidates = parameter_owners.iter().filter_map(|owner| {
        (native_stream(&owner.id) == native_stream(&scope.id)
            && owner.scope_record_index == scope.record_index
            && owner.local_ordinal == 1
            && owner.evaluated_value.is_finite()
            && owner.evaluated_value > 0.0)
            .then_some((
                owner.evaluated_value,
                owner.record_index,
                owner.evaluated_value_offset,
            ))
    });
    let mut angle_candidates = owner_angle_candidates.collect::<Vec<_>>();
    if angle_candidates.is_empty() {
        angle_candidates.extend(scope.reference_members.iter().filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index)
                && scalar.ordinal == 1
                && scalar.value > 0.0)
                .then_some((scalar.value, *record_index, scalar.value_offset))
        }));
    }
    angle_candidates.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    angle_candidates.dedup();
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

/// Join a Mirror scope's two operand groups, fixed parameters, compact feature
/// reference, and `WorkPlane` reference into one exact construction.
pub fn bind_mirror_constructions(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    groups: &[crate::records::DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    owners: &[DesignParameterOwner],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for index in 0..scopes.len() {
        if design_feature_family(&scopes[index].kind) != Some(DesignFeatureFamily::Mirror) {
            continue;
        }
        let Some(stream) = native_stream(&scopes[index].id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope_record_index = scopes[index].record_index;
        let scope_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope_record_index
            })
            .collect::<Vec<_>>();
        let seed_groups = scope_groups
            .iter()
            .copied()
            .filter(|group| group.role == 0x0000_0008_0000_0000)
            .collect::<Vec<_>>();
        let plane_groups = scope_groups
            .iter()
            .copied()
            .filter(|group| group.role == 0x0000_0005_0000_0000)
            .collect::<Vec<_>>();
        let ([seed_group], [plane_group]) = (seed_groups.as_slice(), plane_groups.as_slice())
        else {
            continue;
        };
        let [plane_member] = plane_group.members.as_slice() else {
            continue;
        };
        let Some(plane_header) = headers.get(&(stream, *plane_member)) else {
            continue;
        };
        let Some((plane_reference, plane_reference_offset)) =
            compact_feature_reference(bytes, plane_header)
        else {
            continue;
        };
        let plane_scope_record_index = plane_reference.checked_add(1);
        let Some(plane_scope_record_index) = plane_scope_record_index.filter(|record_index| {
            scopes.iter().any(|scope| {
                native_stream(&scope.id) == Some(stream)
                    && scope.record_index == *record_index
                    && scope.kind == "WorkPlane"
                    && scope.work_plane_transform.is_some()
            })
        }) else {
            continue;
        };
        let seed_feature = match seed_group.members.as_slice() {
            [member] => headers
                .get(&(stream, *member))
                .and_then(|header| compact_feature_reference(bytes, header))
                .filter(|(record_index, _)| {
                    scopes.iter().any(|scope| {
                        native_stream(&scope.id) == Some(stream)
                            && scope.record_index == *record_index
                    })
                }),
            _ => None,
        };
        let scope_owners = owners
            .iter()
            .filter(|owner| {
                native_stream(&owner.id) == Some(stream)
                    && owner.scope_record_index == scope_record_index
            })
            .collect::<Vec<_>>();
        let count = scope_owners
            .iter()
            .copied()
            .filter(|owner| {
                owner.local_ordinal == 0
                    && owner.evaluated_value == 2.0
                    && owner.evaluated_value.is_finite()
            })
            .collect::<Vec<_>>();
        let tolerance = scope_owners
            .iter()
            .copied()
            .filter(|owner| {
                owner.local_ordinal == 1
                    && owner.evaluated_value.is_finite()
                    && owner.evaluated_value > 0.0
            })
            .collect::<Vec<_>>();
        let ([count], [tolerance]) = (count.as_slice(), tolerance.as_slice()) else {
            continue;
        };
        scopes[index].mirror_construction = Some(DesignMirrorConstruction {
            count: 2,
            count_record_index: count.record_index,
            count_offset: count.evaluated_value_offset,
            stitch_tolerance: tolerance.evaluated_value,
            stitch_tolerance_record_index: tolerance.record_index,
            stitch_tolerance_offset: tolerance.evaluated_value_offset,
            seed_group_record_index: seed_group.record_index,
            plane_group_record_index: plane_group.record_index,
            seed_feature_scope_record_index: seed_feature.map(|(record_index, _)| record_index),
            seed_feature_reference_offset: seed_feature.map(|(_, offset)| offset),
            plane_scope_record_index,
            plane_reference_offset,
        });
    }
    Ok(())
}

fn compact_feature_reference(bytes: &[u8], header: &DesignRecordHeader) -> Option<(u32, u64)> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || u32_at(bytes, start + 22)? != header.record_index.checked_add(3)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || u32_at(bytes, start + 32)? != 1
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !crate::bytes::is_guid_relaxed(&asset_id)
        || !crate::bytes::is_guid_relaxed(&context_id)
        || u32_at(bytes, after_context_id)? != 2
        || bytes.get(after_context_id + 4..after_context_id + 8)? != [0; 4]
    {
        return None;
    }
    let paired_at = next_indexed_record_offset(bytes, after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + 11)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + 11)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + 11)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 11)?;
    for (offset, expected) in [
        (paired_at, header.record_index),
        (nested_one_at, header.record_index.checked_add(1)?),
        (nested_two_at, header.record_index.checked_add(2)?),
        (identity_at, header.record_index.checked_add(3)?),
        (next_at, header.record_index.checked_add(4)?),
    ] {
        let (_, after_tag) = lp_ascii_filtered(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    if identity_at.checked_add(29)? != next_at
        || bytes.get(identity_at + 11..identity_at + 21)? != [0; 10]
        || bytes.get(identity_at + 25..identity_at + 29)? != [0; 4]
    {
        return None;
    }
    Some((
        u32_at(bytes, identity_at + 21)?,
        u64::try_from(identity_at + 21).ok()?,
    ))
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
        || u32_at(bytes, start + 89)? == 0
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
                    && (marked_record_reference(bytes, start + 24).is_none()
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
        DesignFeatureFamily::Thicken if scope.reference_members.len() == 3 => {
            let reference_offset = match parameter_scope_payload_length(scope) {
                Some(281)
                    if matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46) == Some(&1) =>
                {
                    46
                }
                Some(287) if bytes.get(start + 47) == Some(&1) => 47,
                _ => return None,
            };
            let thickness_record_index = u32_at(bytes, start + reference_offset + 1)?;
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
                ("296" | "362", 253) | ("349", 254 | 274) | ("368", 254)
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
        || scope
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent)
            != Some(DesignExtrudeExtent::OneSidedDistance)
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
    let group = |tangency_lane: Option<&(u32, FixedScalarFrame)>,
                 radius_lanes: Vec<&(u32, FixedScalarFrame)>,
                 parameter_lanes: Vec<&(u32, FixedScalarFrame)>| {
        let tangency_weight = match tangency_lane {
            Some((record_index, scalar)) if scalar.value > 0.0 => {
                Some(crate::records::DesignFixedFilletTangencyWeight {
                    value: scalar.value,
                    record_index: *record_index,
                    value_offset: scalar.value_offset,
                })
            }
            Some(_) => return None,
            None => None,
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
        Some(DesignFixedFilletGroup {
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
    };
    let groups = if lanes.len() == 1 {
        vec![group(None, vec![&lanes[0]], Vec::new())?]
    } else if lanes.len() % 2 == 0 {
        lanes
            .chunks_exact(2)
            .map(|pair| group(Some(&pair[0]), vec![&pair[1]], Vec::new()))
            .collect::<Option<Vec<_>>>()?
    } else {
        let radius_lanes = lanes[1..3]
            .iter()
            .chain(lanes[3..].chunks_exact(2).map(|pair| &pair[0]))
            .collect::<Vec<_>>();
        let parameter_lanes = lanes[3..]
            .chunks_exact(2)
            .map(|pair| &pair[1])
            .collect::<Vec<_>>();
        vec![group(Some(&lanes[0]), radius_lanes, parameter_lanes)?]
    };
    Some(DesignFixedFilletParameters { groups })
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
    if !(1..=2).contains(&lanes.len())
        || lanes
            .iter()
            .enumerate()
            .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
        || lanes.iter().any(|(_, scalar)| scalar.value <= 0.0)
    {
        return None;
    }
    let mut distances =
        lanes
            .into_iter()
            .map(|(record_index, scalar)| DesignFixedChamferDistance {
                value: scalar.value,
                record_index,
                value_offset: scalar.value_offset,
            });
    let first = distances.next()?;
    Some(match distances.next() {
        Some(second) => DesignFixedChamferParameters::TwoDistances { first, second },
        None => DesignFixedChamferParameters::EqualDistance { distance: first },
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

pub(crate) fn exact_combine_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignCombineOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Combine)
        || scope.reference_members.len() < 4
        || !scope.reference_members.len().is_multiple_of(2)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if bytes.get(start + 11..start + 19)? != [0; 8]
        || bytes.get(start + 24) != Some(&0)
        || bytes.get(start + 26..start + 33)? != [0; 7]
    {
        return None;
    }
    let operation = match u32_at(bytes, start + 20)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        _ => return None,
    };
    let keep_tools = match bytes.get(start + 25)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let mut target = None;
    let mut tools = Vec::with_capacity(scope.reference_members.len() / 2);
    for pair in scope.reference_members.chunks_exact(2) {
        let [operation_record_index, selection_record_index] = pair else {
            return None;
        };
        let operation_frames = indexed_record_pairs(bytes, *operation_record_index);
        let [(operation_at, operation_end)] = operation_frames.as_slice() else {
            return None;
        };
        match combine_operation_identity_role(
            bytes.get(*operation_at..*operation_end)?,
            *selection_record_index,
        )? {
            CombineOperandRole::Target => {
                if target.replace(*selection_record_index).is_some() {
                    return None;
                }
            }
            CombineOperandRole::Tool => tools.push(*selection_record_index),
        }
        let selection_frames = indexed_record_pairs(bytes, *selection_record_index);
        let [(selection_at, selection_end)] = selection_frames.as_slice() else {
            return None;
        };
        if !contains_consecutive_guid_pair(bytes.get(*selection_at..*selection_end)?) {
            return None;
        }
    }
    let target = target?;
    if tools.is_empty() {
        return None;
    }
    let mut body_selection_record_indexes = Vec::with_capacity(tools.len() + 1);
    body_selection_record_indexes.push(target);
    body_selection_record_indexes.extend(tools);
    Some(DesignCombineOperation {
        operation,
        operation_offset: u64::try_from(start + 20).ok()?,
        keep_tools,
        keep_tools_offset: u64::try_from(start + 25).ok()?,
        body_selection_record_indexes,
    })
}

#[derive(Clone, Copy)]
enum CombineOperandRole {
    Target,
    Tool,
}

fn combine_operation_identity_role(
    frame: &[u8],
    selection_record_index: u32,
) -> Option<CombineOperandRole> {
    let selection_reference = selection_record_index.to_le_bytes();
    if frame.get(11..21)? == [0; 10]
        && u32_at(frame, 21)? == 1
        && frame.get(25) == Some(&1)
        && frame.get(26..30)? == selection_reference
        && frame.get(30..36)? == [0; 6]
    {
        return Some(CombineOperandRole::Target);
    }
    if frame.get(11..20)? != [0; 9] || frame.get(20) != Some(&1) || u32_at(frame, 21)? != 1 {
        return None;
    }
    let (property, after_property) = lp_ascii_filtered(frame, 25, 0..=2000, u8::is_ascii_graphic)?;
    let (property_type, after_property_type) =
        lp_ascii_filtered(frame, after_property, 0..=2000, u8::is_ascii_graphic)?;
    let count_at = after_property_type.checked_add(8)?;
    if property != "DcFeatureOperationIdFlag"
        || property_type != "IntrinsicMetaTypeuint64"
        || u32_at(frame, count_at)? != 1
        || frame.get(count_at + 4) != Some(&1)
        || frame.get(count_at + 5..count_at + 9)? != selection_reference
        || frame.get(count_at + 9..count_at + 15)? != [0; 6]
    {
        return None;
    }
    Some(CombineOperandRole::Tool)
}

pub(crate) fn exact_draft_operation(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignDraftOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Draft)
        || scope.frame_length != 361
        || scope.reference_members.len() != 7
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
    let [(angle_record_index, angle), (opposite_angle_record_index, opposite)] = lanes.as_slice()
    else {
        return None;
    };
    if angle.ordinal != 0
        || opposite.ordinal != 1
        || !angle.value.is_finite()
        || angle.value == 0.0
        || opposite.value != 0.0
        || *angle_record_index != scope.reference_members[0]
        || *opposite_angle_record_index != scope.reference_members[1]
    {
        return None;
    }
    Some(DesignDraftOperation {
        angle: angle.value,
        angle_record_index: *angle_record_index,
        angle_offset: angle.value_offset,
        opposite_angle_record_index: *opposite_angle_record_index,
        opposite_angle_offset: opposite.value_offset,
    })
}

fn contains_consecutive_guid_pair(bytes: &[u8]) -> bool {
    (0..bytes.len()).any(|at| {
        lp_utf16_bounded(bytes, at, 1..=256)
            .filter(|(first, _)| crate::bytes::is_guid_relaxed(first))
            .and_then(|(_, after_first)| lp_utf16_bounded(bytes, after_first, 1..=256))
            .is_some_and(|(second, _)| crate::bytes::is_guid_relaxed(&second))
    })
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

pub(crate) fn parameter_scope_tail_length_is_valid(kind: &str, tail_length: usize) -> bool {
    if kind == "CopyPasteBodies" {
        tail_length == 110
    } else {
        matches!(tail_length, 77 | 78 | 87)
    }
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
            if parameter_scope_tail_length_is_valid(&kind, tail_length)
                && kind.chars().all(|character| !character.is_control())
            {
                candidates.push((at, end, tail_length, kind));
            }
        }
    }
    let [(kind_at, kind_end, tail_length, kind)] = candidates.as_slice() else {
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
        kind_end.checked_add(match (kind.as_str(), *tail_length) {
            ("CopyPasteBodies", _) => 53,
            (_, 87) => 41,
            _ => 31,
        })?;
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
    let extrude_prologue = if family == Some(DesignFeatureFamily::Extrude) {
        // The generic scope envelope is independently self-delimiting. An
        // unrecognized Extrude prologue therefore withholds only the typed
        // fields, not the scope and its ordered reference table.
        exact_extrude_prologue(bytes, start)
    } else {
        None
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
        let operation = match (kind.as_str(), u32_at(bytes, operation_offset)?) {
            ("SpirePrimitive", 1) => DesignExtrudeOperation::Join,
            ("SpirePrimitive", 2) => DesignExtrudeOperation::Cut,
            ("SpirePrimitive", 3) => DesignExtrudeOperation::Intersect,
            ("SpirePrimitive", 4) | ("CoilPrimitive", 1) => DesignExtrudeOperation::NewBody,
            _ => return None,
        };
        let clockwise_offset = start.checked_add(24)?;
        let clockwise = match bytes.get(clockwise_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let structural_constant = match kind.as_str() {
            "SpirePrimitive" => 2,
            "CoilPrimitive" => 4,
            _ => return None,
        };
        if u32_at(bytes, start.checked_add(26)?)? != structural_constant {
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
        let section = match (kind.as_str(), u32_at(bytes, section_offset)?) {
            ("SpirePrimitive", 0) => DesignCoilSection::Circular,
            ("SpirePrimitive", 1) => DesignCoilSection::Square,
            ("SpirePrimitive", 2) | ("CoilPrimitive", 1) => DesignCoilSection::ExternalTriangle,
            ("SpirePrimitive", 3) => DesignCoilSection::InternalTriangle,
            _ => return None,
        };
        let section_placement_offset = start.checked_add(107)?;
        let section_placement = match (kind.as_str(), u32_at(bytes, section_placement_offset)?) {
            ("SpirePrimitive", 4) | ("CoilPrimitive", 3) => DesignCoilSectionPlacement::Inside,
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
        extrude_prologue,
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
        combine_operation: None,
        draft_operation: None,
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
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag,
        paired_byte_offset: paired_at as u64,
    })
}

fn exact_extrude_prologue(bytes: &[u8], start: usize) -> Option<DesignExtrudePrologue> {
    exact_current_extrude_prologue(bytes, start)
        .or_else(|| exact_legacy_shifted_extrude_prologue(bytes, start))
}

fn exact_current_extrude_prologue(bytes: &[u8], start: usize) -> Option<DesignExtrudePrologue> {
    let direct_offset = start.checked_add(28)?;
    let reference = if bytes.get(start.checked_add(25)?) == Some(&1) {
        let reference_record_index_offset = start.checked_add(26)?;
        let record_index = u32_at(bytes, reference_record_index_offset)?;
        let prefix_tail = start.checked_add(30)?;
        let candidates = [start.checked_add(37)?, start.checked_add(38)?]
            .into_iter()
            .filter(|operation_offset| {
                bytes
                    .get(prefix_tail..*operation_offset)
                    .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
                    && matches!(u32_at(bytes, *operation_offset), Some(1..=4))
                    && matches!(
                        (
                            u32_at(bytes, operation_offset.saturating_add(4)),
                            u32_at(bytes, operation_offset.saturating_add(8))
                        ),
                        (Some(1), Some(1 | 2)) | (Some(2), Some(0)) | (Some(3), Some(2))
                    )
                    && matches!(bytes.get(operation_offset.saturating_add(12)), Some(0 | 1))
                    && bytes.get(operation_offset.saturating_add(13)) == Some(&1)
                    && matches!(bytes.get(operation_offset.saturating_add(14)), Some(0..=2))
            })
            .collect::<Vec<_>>();
        let [operation_offset] = candidates.as_slice() else {
            return None;
        };
        let trailing_zero_count = u8::try_from(operation_offset.checked_sub(prefix_tail)?).ok()?;
        Some((
            *operation_offset,
            DesignExtrudePrologueReference {
                record_index,
                record_index_offset: reference_record_index_offset as u64,
                trailing_zero_count,
            },
        ))
    } else {
        None
    };
    let (operation_offset, reference) = reference
        .map_or((direct_offset, None), |(offset, reference)| {
            (offset, Some(reference))
        });
    let operation = match u32_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let side_offset = operation_offset.checked_add(4)?;
    let termination_offset = operation_offset.checked_add(8)?;
    let extent_discriminators = [
        u32_at(bytes, side_offset)?,
        u32_at(bytes, termination_offset)?,
    ];
    let extent = match extent_discriminators {
        [1, 1] => DesignExtrudeExtent::OneSidedToFace,
        [1, 2] => DesignExtrudeExtent::OneSidedDistance,
        [2, 0] => DesignExtrudeExtent::TwoSidedDistance,
        [3, 2] => DesignExtrudeExtent::SymmetricDistance,
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
    Some(DesignExtrudePrologue::ReferenceAware {
        reference,
        operation,
        operation_offset: operation_offset as u64,
        extent_discriminators,
        extent,
        extent_discriminator_offsets: [side_offset as u64, termination_offset as u64],
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        start,
        start_offset: start_offset as u64,
    })
}

fn exact_legacy_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
) -> Option<DesignExtrudePrologue> {
    let operation_offset = start.checked_add(27)?;
    let operation = match u32_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let first_extent_offset = operation_offset.checked_add(4)?;
    let second_extent_offset = operation_offset.checked_add(8)?;
    let extent_discriminators = [
        u32_at(bytes, first_extent_offset)?,
        u32_at(bytes, second_extent_offset)?,
    ];
    let extent = match extent_discriminators {
        [1, 1] => Some(DesignExtrudeExtent::OneSidedToFace),
        [1, 2] => Some(DesignExtrudeExtent::OneSidedDistance),
        [2, 0] => Some(DesignExtrudeExtent::TwoSidedDistance),
        [3, 2] => Some(DesignExtrudeExtent::SymmetricDistance),
        _ => None,
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
    Some(DesignExtrudePrologue::LegacyShifted {
        operation,
        operation_offset: operation_offset as u64,
        extent_discriminators,
        extent,
        extent_discriminator_offsets: [first_extent_offset as u64, second_extent_offset as u64],
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        start,
        start_offset: start_offset as u64,
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

#[cfg(test)]
mod mirror_tests {
    use super::*;

    fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> usize {
        let start = bytes.len();
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        start
    }

    fn utf16(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(
            &u32::try_from(value.encode_utf16().count())
                .expect("test GUID length fits u32")
                .to_le_bytes(),
        );
        for unit in value.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    #[test]
    fn compact_mirror_reference_uses_the_identity_record_lane() {
        let record_index = 40;
        let reference = 17_u32;
        let mut bytes = Vec::new();
        let start = indexed_header(&mut bytes, *b"320", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        utf16(&mut bytes, "dfa12ed5-41e3-47c2-947d-286843e235df");
        utf16(&mut bytes, "15afb570-2968-417f-8485-96c81b2d332f");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        indexed_header(&mut bytes, *b"259", record_index);
        indexed_header(&mut bytes, *b"306", record_index + 1);
        indexed_header(&mut bytes, *b"291", record_index + 2);
        let identity = indexed_header(&mut bytes, *b"428", record_index + 3);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        indexed_header(&mut bytes, *b"457", record_index + 4);
        let header = DesignRecordHeader {
            id: String::new(),
            record_index,
            class_tag: "320".into(),
            byte_offset: start as u64,
        };

        assert_eq!(
            compact_feature_reference(&bytes, &header),
            Some((reference, (identity + 21) as u64))
        );
        bytes[identity + 20] = 1;
        assert_eq!(compact_feature_reference(&bytes, &header), None);
    }
}
