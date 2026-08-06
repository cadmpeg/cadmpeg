// SPDX-License-Identifier: Apache-2.0
//! Parse parameter scopes and exact feature-construction frames.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded, take_reference};
use crate::container::{role, ContainerScan};
use crate::design::decode::operands::{
    parse_construction_operand_group, ConstructionOperandGroupParse,
};
use crate::design::decode::sketch::{
    next_indexed_record_offset, valid_sketch_transform, IndexedRecordOffsets,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::records::{
    DesignAssemblyAlignment, DesignAssemblyOperandFrame, DesignAssemblyOperandPath,
    DesignBaseFeatureConstruction, DesignBaseFlangeOperation, DesignBendPosition,
    DesignCircularPatternConstruction, DesignCoilExtent, DesignCoilSection,
    DesignCoilSectionPlacement, DesignCombineOperation, DesignComponentInsertConstruction,
    DesignComponentOccurrence, DesignComponentPatternOccurrences, DesignCopyPasteBodiesOperation,
    DesignCopyPasteComponentOperation, DesignDirectFaceOperation, DesignDraftOperation,
    DesignEdgeFlangeHeightExtent, DesignEdgeFlangeOperation, DesignEntityHeader,
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
    DesignExtrudePrologueReference, DesignExtrudeStart, DesignFixedChamferDistance,
    DesignFixedChamferParameters, DesignFixedExtrudeDistance, DesignFixedExtrudeParameters,
    DesignFixedExtrudeScalar, DesignFixedFilletGroup, DesignFixedFilletParameters,
    DesignHemOperation, DesignHemParameterOwners, DesignMirrorConstruction, DesignMoveOperation,
    DesignParameter, DesignParameterOwner, DesignParameterScope, DesignPathFeatureConstruction,
    DesignRecordHeader, DesignRectangularPatternConstruction, DesignRectangularPatternInstances,
    DesignRuledSurfaceCorner, DesignRuledSurfaceMethod, DesignRuledSurfaceOperation,
    DesignScaleOperation, DesignSheetMetalHeightDatum, DesignSolidPrimitive,
    DesignSurfaceExtendMethod, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfaceOffsetSupport, DesignSurfaceStitchOperation, DesignThreadConstruction,
    DesignWorkAxisConstruction,
};
use cadmpeg_core::le::{f64_at, f64s_at, u32_at, u64_at as read_u64};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub fn decode_parameter_scopes(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
    types: &[crate::records::DesignType],
    parameters: &[DesignParameter],
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
        let records = IndexedRecordOffsets::build(bytes);
        let stream_types =
            crate::design::decode::sketch::stream_types_by_entity(types, &entry.name);
        let stream_scope_start = out.len();
        for header in parameter_scope_candidate_headers(bytes, &records) {
            let Some(mut scope) = parse_parameter_scope(bytes, &records, &header) else {
                continue;
            };
            scope.id = ids::native_design_parameter_scope_id(&entry.name, scope.byte_offset);
            bind_coil_extent_from_parameters(&mut scope, parameters, parameter_owners);
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
                                || !entity.in_sketch_module()
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
                if let Some(frame) = exact_work_plane_frame(bytes, &records, &scope) {
                    scope.work_plane_transform = Some(frame.transform);
                    scope.work_plane_transform_offset = Some(frame.transform_offset);
                    if let Some((reference, reference_offset)) = frame.reference {
                        scope.work_plane_reference = Some(reference);
                        scope.work_plane_reference_offset = Some(reference_offset);
                    }
                }
            }
            if let Some(construction) = exact_work_axis_construction(bytes, &records, &scope) {
                scope.work_axis_construction = Some(construction);
            }
            if scope.kind == "JointOrigin" {
                if let Some(frame) = exact_joint_origin_frame(bytes, &records, &scope) {
                    scope.joint_origin_transform = Some(frame.transform);
                    scope.joint_origin_transform_offset = Some(frame.transform_offset);
                    if let Some((reference, reference_offset)) = frame.reference {
                        scope.joint_origin_reference = Some(reference);
                        scope.joint_origin_reference_offset = Some(reference_offset);
                    }
                }
            }
            if let Some(frame) = exact_work_point_position(bytes, &records, &scope, &stream_types) {
                scope.work_point_position = Some(frame.position);
                scope.work_point_position_offset = Some(frame.position_offset);
                scope.work_point_reference_type = Some(frame.reference_type);
                scope.work_point_input_record_indices = frame.input_record_indices;
            }
            scope.solid_primitive =
                exact_solid_primitive(bytes, &records, &scope, parameter_owners);
            scope.direct_face_operation = exact_direct_face_operation(bytes, &records, &scope);
            scope.move_operation = exact_move_operation(bytes, &records, &scope);
            scope.scale_operation = exact_scale_operation(bytes, &scope);
            scope.surface_extend_operation =
                exact_surface_extend_operation(bytes, &records, &scope);
            scope.surface_offset_operation =
                exact_surface_offset_operation(bytes, &records, &scope);
            scope.fixed_extrude_parameters =
                exact_fixed_extrude_parameters(bytes, &records, &scope);
            scope.fixed_fillet_parameters = exact_fixed_fillet_parameters(bytes, &records, &scope);
            scope.fixed_chamfer_parameters =
                exact_fixed_chamfer_parameters(bytes, &records, &scope, parameter_owners);
            scope.path_feature_construction =
                exact_path_feature_construction(bytes, &records, &scope, parameter_owners);
            scope.combine_operation = exact_combine_operation(bytes, &records, &scope);
            scope.thread_construction = exact_thread_construction(bytes, &scope);
            scope.draft_operation =
                exact_draft_operation_with_owners(bytes, &records, &scope, parameter_owners);
            scope.circular_pattern_construction = exact_circular_pattern_construction_with_owners(
                bytes,
                &records,
                &scope,
                parameter_owners,
            );
            scope.rectangular_pattern_construction =
                exact_rectangular_pattern_construction(bytes, &records, &scope, parameter_owners);
            scope.assembly_alignment =
                exact_assembly_alignment(bytes, &records, &scope, parameter_owners);
            scope.component_insert_construction =
                exact_component_insert_construction(bytes, &records, &scope);
            scope.copy_paste_component_operation = exact_copy_paste_component_operation(
                bytes,
                &records,
                &scope,
                component_occurrences,
            );
            bind_component_pattern_occurrences(&mut scope, component_occurrences);
            scope.copy_paste_bodies_operation =
                exact_copy_paste_bodies_operation(bytes, &records, &scope);
            scope.base_feature_construction = exact_base_feature_construction(bytes, &scope);
            out.push(scope);
        }
        bind_joint_origin_frames_from_assemblies(bytes, &mut out[stream_scope_start..]);
    }
    out.sort_by_key(|scope| scope.id.clone());
    out.dedup_by_key(|scope| scope.id.clone());
    Ok(out)
}

pub(crate) fn exact_thread_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignThreadConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    if scope.kind != "Thread"
        || scope.frame_length != 449
        || scope.paired_class_tag != "258"
        || scope.reference_members.len() != 4
        || bytes.get(start + 11..start + 21)? != [0; 10]
        || f64_at(bytes, start + 21)?.to_bits() != 60.0f64.to_bits()
        || bytes.get(start + 29..start + 34)? != [1, 2, 0, 0, 0]
        || bytes.get(start + 34..start + 38)? != [0x36, 0, 0x67, 0]
    {
        return None;
    }
    parse_thread_payload(bytes, start, scope.reference_members[0])
}

pub(crate) fn parse_thread_payload(
    bytes: &[u8],
    start: usize,
    face_group_record_index: u32,
) -> Option<DesignThreadConstruction> {
    let (designation, after_designation) = lp_utf16_bounded(bytes, start + 38, 1..=128)?;
    let (nominal, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
    let (profile, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
    if after_profile != start + 108 || bytes.get(start + 108..start + 113)? != [0, 1, 0, 0, 0] {
        return None;
    }
    let nominal_size = nominal.parse::<f64>().ok()?;
    let major_diameter = f64_at(bytes, start + 113)?;
    let minor_diameter = f64_at(bytes, start + 121)?;
    let pitch = (bytes.get(start + 129) == Some(&1)).then(|| f64_at(bytes, start + 130))??;
    let pitch_diameter = f64_at(bytes, start + 138)?;
    if bytes.get(start + 146..start + 148)? != [0, 1]
        || ![
            nominal_size,
            major_diameter,
            minor_diameter,
            pitch,
            pitch_diameter,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
        || !(minor_diameter < pitch_diameter && pitch_diameter < major_diameter)
    {
        return None;
    }
    Some(DesignThreadConstruction {
        designation,
        nominal_size,
        profile,
        major_diameter,
        minor_diameter,
        pitch,
        pitch_diameter,
        face_group_record_index,
    })
}

pub(crate) fn bind_joint_origin_frames_from_assemblies(
    bytes: &[u8],
    scopes: &mut [DesignParameterScope],
) {
    let mut candidates = Vec::new();
    let mut envelopes = Vec::new();
    for scope in scopes.iter() {
        if scope.kind != "Assemble" {
            continue;
        }
        if let Some(frames) = scope
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.operand_frames.as_ref())
        {
            for frame in frames {
                candidates.push((
                    frame.reference_record_index,
                    frame.transform,
                    frame.transform_offset,
                    None,
                ));
            }
        }
        if let Some((joint_origin, frame)) = exact_single_joint_origin_frame(bytes, scope) {
            envelopes.push((scope.record_index, joint_origin));
            candidates.push((
                joint_origin,
                frame.transform,
                frame.transform_offset,
                frame.reference,
            ));
        }
    }
    for scope in scopes
        .iter_mut()
        .filter(|scope| scope.kind == "JointOrigin" && scope.joint_origin_transform.is_none())
    {
        let mut matches = candidates
            .iter()
            .filter(|(record_index, ..)| *record_index == scope.record_index);
        let Some((_, transform, transform_offset, reference)) = matches.next() else {
            continue;
        };
        if matches.any(|(_, other_transform, _, other_reference)| {
            other_transform != transform
                || other_reference.map(|(record_index, _)| record_index)
                    != reference.map(|(record_index, _)| record_index)
        }) {
            continue;
        }
        scope.joint_origin_transform = Some(*transform);
        scope.joint_origin_transform_offset = Some(*transform_offset);
        if let Some((record_index, offset)) = reference {
            scope.joint_origin_reference = Some(*record_index);
            scope.joint_origin_reference_offset = Some(*offset);
        }
    }
    let resolved_origins = scopes
        .iter()
        .filter(|scope| scope.kind == "JointOrigin" && scope.joint_origin_transform.is_some())
        .map(|scope| scope.record_index)
        .collect::<HashSet<_>>();
    for (assembly_record_index, joint_origin_record_index) in envelopes {
        if !resolved_origins.contains(&joint_origin_record_index) {
            continue;
        }
        let mut assemblies = scopes.iter_mut().filter(|scope| {
            scope.kind == "Assemble" && scope.record_index == assembly_record_index
        });
        let Some(assembly) = assemblies.next() else {
            continue;
        };
        if assemblies.next().is_some() {
            continue;
        }
        if let Some(alignment) = assembly.assembly_alignment.as_mut() {
            alignment.joint_origin_scope_record_index = Some(joint_origin_record_index);
        }
    }
}

fn exact_single_joint_origin_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(u32, ScopePlacementFrame)> {
    if scope.kind != "Assemble"
        || scope.class_tag != "276"
        || scope.paired_class_tag != "258"
        || scope.frame_length != 604
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if usize::try_from(scope.paired_byte_offset).ok()? != start.checked_add(604)?
        || bytes.get(start + 11..start + 24)? != [0; 13]
        || bytes.get(start + 29..start + 36)? != [0; 7]
        || bytes.get(start + 169..start + 175)? != [0; 6]
        || u32_at(bytes, start + 175)? != 1
    {
        return None;
    }
    let reference_record_index = marked_record_reference(bytes, start + 24)?;
    let joint_origin_record_index = marked_record_reference(bytes, start + 164)?;
    if reference_record_index == joint_origin_record_index {
        return None;
    }
    let transform = rigid_transform_at(bytes, start + 36)?;
    Some((
        joint_origin_record_index,
        ScopePlacementFrame {
            transform,
            transform_offset: u64::try_from(start + 36).ok()?,
            reference: Some((reference_record_index, u64::try_from(start + 25).ok()?)),
        },
    ))
}

pub(crate) fn exact_surface_extend_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceExtendOperation> {
    let operation = exact_surface_boundary_operation(
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceExtend,
        8,
    )?;
    if operation.distance <= 0.0 {
        return None;
    }
    let method = match operation.mode {
        0 => DesignSurfaceExtendMethod::Natural,
        1 => DesignSurfaceExtendMethod::Tangent,
        2 => DesignSurfaceExtendMethod::Perpendicular,
        _ => return None,
    };
    Some(DesignSurfaceExtendOperation {
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
    })
}

pub(crate) fn exact_surface_offset_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceOffsetOperation> {
    if let Some(operation) = exact_surface_offset_face_groups(bytes, records, scope) {
        return Some(operation);
    }
    let operation = exact_surface_boundary_operation(
        bytes,
        records,
        scope,
        DesignFeatureFamily::SurfaceOffset,
        65,
    )?;
    (operation.mode == 1).then_some(DesignSurfaceOffsetOperation {
        distance: operation.distance,
        distance_offset: operation.distance_offset,
        distance_record_index: operation.distance_record_index,
        support: DesignSurfaceOffsetSupport::BoundaryCarrier {
            boundary_mode: operation.mode,
            boundary_mode_offset: operation.mode_offset,
            boundary_record_index: operation.boundary_record_index,
            boundary_reference_record_index: operation.boundary_reference_record_index,
            boundary_reference_offset: operation.boundary_reference_offset,
            edge_record_indices: operation.edge_record_indices,
            tolerance: operation.tolerance,
            tolerance_offset: operation.tolerance_offset,
        },
    })
}

fn exact_surface_offset_face_groups(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignSurfaceOffsetOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::SurfaceOffset) {
        return None;
    }
    let [distance_record_index, support_references @ ..] = scope.reference_members.as_slice()
    else {
        return None;
    };
    if support_references.is_empty() {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, *distance_record_index)?;
    if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
        return None;
    }

    let mut group_record_indices = Vec::new();
    let mut covered_references = HashSet::new();
    for (scope_reference_ordinal, record_index) in
        scope.reference_members.iter().copied().enumerate().skip(1)
    {
        let group = exact_construction_operand_group(
            bytes,
            records,
            scope,
            u32::try_from(scope_reference_ordinal).ok()?,
            record_index,
        );
        let Some(group) = group else {
            continue;
        };
        if group.role != 0x0000_0041_0000_0000
            || group.frame.opaque_index != 252
            || group.members.is_empty()
            || !covered_references.insert(group.record_index)
        {
            return None;
        }
        for member in &group.members {
            if *member == *distance_record_index
                || !support_references.contains(member)
                || !covered_references.insert(*member)
            {
                return None;
            }
        }
        group_record_indices.push(group.record_index);
    }
    if group_record_indices.is_empty() || covered_references.len() != support_references.len() {
        return None;
    }

    Some(DesignSurfaceOffsetOperation {
        distance: scalar.value,
        distance_offset: scalar.value_offset,
        distance_record_index: *distance_record_index,
        support: DesignSurfaceOffsetSupport::FaceGroups {
            group_record_indices,
        },
    })
}

fn exact_construction_operand_group(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    record_index: u32,
) -> Option<crate::records::DesignConstructionOperandGroup> {
    let mut candidates = Vec::new();
    for (start, _) in records.frames(record_index) {
        let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if after_tag != start + 7 {
            continue;
        }
        let header = DesignRecordHeader {
            id: String::new(),
            record_index,
            class_tag: class_tag.clone(),
            byte_offset: u64::try_from(start).ok()?,
        };
        if let ConstructionOperandGroupParse::Complete(group) =
            parse_construction_operand_group(bytes, scope, scope_reference_ordinal, &header)
        {
            candidates.push(*group);
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

#[derive(Clone)]
struct ExactSurfaceBoundaryOperation {
    distance: f64,
    distance_offset: u64,
    distance_record_index: u32,
    mode: u32,
    mode_offset: u64,
    boundary_record_index: u32,
    boundary_reference_record_index: u32,
    boundary_reference_offset: u64,
    edge_record_indices: Vec<u32>,
    tolerance: f64,
    tolerance_offset: u64,
}

fn exact_surface_boundary_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    family: DesignFeatureFamily,
    boundary_kind: u32,
) -> Option<ExactSurfaceBoundaryOperation> {
    if design_feature_family(&scope.kind) != Some(family) {
        return None;
    }
    let [distance_record_index, boundary_record_index, edge_record_indices @ ..] =
        scope.reference_members.as_slice()
    else {
        return None;
    };
    if edge_record_indices.is_empty() {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, *distance_record_index)?;
    if scalar.owner_record_index != Some(scope.record_index)
        || scalar.ordinal != 0
        || records
            .frames(*distance_record_index)
            .filter(|(start, end)| {
                end.checked_sub(*start) == Some(104)
                    && lp_ascii_filtered(bytes, *start, 0..=2000, u8::is_ascii_graphic).is_some_and(
                        |(class_tag, after_tag)| {
                            after_tag == *start + 7
                                && class_tag.len() == 3
                                && class_tag.bytes().all(|byte| byte.is_ascii_digit())
                        },
                    )
                    && bytes.get(*start + 11..*start + 19) == Some(&[0; 8])
                    && bytes.get(*start + 19..*start + 24) == Some(&[1, 1, 0, 0, 0])
                    && marked_record_reference(bytes, *start + 24) == Some(scope.record_index)
                    && bytes.get(*start + 29..*start + 35) == Some(&[0; 6])
                    && bytes.get(*start + 35..*start + 40) == Some(&[0; 5])
                    && marked_record_reference(bytes, *start + 48)
                        == distance_record_index.checked_sub(1)
                    && bytes.get(*start + 53..*start + 59) == Some(&[0; 6])
                    && u32_at(bytes, *start + 59).is_some_and(|value| value != 0)
                    && bytes.get(*start + 63..*start + 67) == Some(&[0; 4])
                    && marked_record_reference(bytes, *start + 67) == Some(scope.record_index)
                    && bytes.get(*start + 72..*start + 78) == Some(&[0; 6])
                    && bytes.get(*start + 78..*start + 81) == Some(&[1, 0, 0])
                    && marked_record_reference(bytes, *start + 81)
                        == distance_record_index.checked_add(1)
                    && bytes.get(*start + 86..*start + 93) == Some(&[0; 7])
                    && marked_record_reference(bytes, *start + 93) == Some(scope.record_index)
                    && bytes.get(*start + 98..*start + 104) == Some(&[0; 6])
            })
            .count()
            != 1
    {
        return None;
    }
    let candidates = records
        .frames(*boundary_record_index)
        .filter_map(|(start, end)| {
            let member_bytes = edge_record_indices.len().checked_mul(11)?;
            let tail = start.checked_add(25)?.checked_add(member_bytes)?;
            (end.checked_sub(start)? == 113usize.checked_add(member_bytes)?).then_some(())?;
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            if after_tag != start + 7
                || class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || bytes.get(start + 11..start + 21)? != [0; 10]
                || u32_at(bytes, start + 21)? != u32::try_from(edge_record_indices.len()).ok()?
                || edge_record_indices
                    .iter()
                    .enumerate()
                    .any(|(ordinal, record_index)| {
                        marked_record_reference(bytes, start + 25 + ordinal * 11)
                            != Some(*record_index)
                    })
                || bytes.get(tail..tail + 2)? != [0; 2]
                || bytes.get(tail + 11..tail + 21)? != [0; 10]
                || u32_at(bytes, tail + 21)? != boundary_kind
                || bytes.get(tail + 25..tail + 35)? != [0; 10]
                || u32_at(bytes, tail + 35)? != 210
                || u32_at(bytes, tail + 47)? != 210
                || marked_record_reference(bytes, tail + 51) != boundary_record_index.checked_add(2)
                || bytes.get(tail + 56..tail + 62)? != [0; 6]
                || bytes.get(tail + 62..tail + 65)? != [1, 0, 0]
                || marked_record_reference(bytes, tail + 65) != boundary_record_index.checked_add(1)
                || bytes.get(tail + 70..tail + 77)? != [0; 7]
                || marked_record_reference(bytes, tail + 77) != Some(scope.record_index)
                || bytes.get(tail + 82..tail + 88)? != [0; 6]
            {
                return None;
            }
            let mode = u32_at(bytes, tail + 2)?;
            let boundary_reference_record_index = marked_record_reference(bytes, tail + 6)?;
            let tolerance = f64_at(bytes, tail + 39)?;
            (tolerance.is_finite() && tolerance > 0.0).then_some(ExactSurfaceBoundaryOperation {
                distance: scalar.value,
                distance_offset: scalar.value_offset,
                distance_record_index: *distance_record_index,
                mode,
                mode_offset: u64::try_from(tail + 2).ok()?,
                boundary_record_index: *boundary_record_index,
                boundary_reference_record_index,
                boundary_reference_offset: u64::try_from(tail + 6).ok()?,
                edge_record_indices: edge_record_indices.to_vec(),
                tolerance,
                tolerance_offset: u64::try_from(tail + 39).ok()?,
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(crate) fn exact_assembly_alignment(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
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
    if lanes
        .iter()
        .enumerate()
        .any(|(ordinal, owner)| owner.local_ordinal != ordinal as u32)
    {
        return None;
    }
    let alignment_lanes = match lanes.len() {
        4 => &lanes[..],
        8 => &lanes[4..],
        10 => &lanes[8..],
        _ => return None,
    };
    let (angle, offset, owner_record_indices, value_offsets) = match alignment_lanes {
        [angle, offset_x, offset_y, offset_z] => (
            *angle,
            [
                offset_x.evaluated_value,
                offset_y.evaluated_value,
                offset_z.evaluated_value,
            ],
            vec![
                angle.record_index,
                offset_x.record_index,
                offset_y.record_index,
                offset_z.record_index,
            ],
            vec![
                angle.evaluated_value_offset,
                offset_x.evaluated_value_offset,
                offset_y.evaluated_value_offset,
                offset_z.evaluated_value_offset,
            ],
        ),
        [angle, axial_offset] => (
            *angle,
            [0.0, 0.0, axial_offset.evaluated_value],
            vec![angle.record_index, axial_offset.record_index],
            vec![
                angle.evaluated_value_offset,
                axial_offset.evaluated_value_offset,
            ],
        ),
        _ => return None,
    };
    if !scope.reference_members.ends_with(&owner_record_indices) {
        return None;
    }
    let mut alignment = DesignAssemblyAlignment {
        angle: angle.evaluated_value,
        offset,
        owner_record_indices,
        value_offsets,
        operand_frames: None,
        operand_paths: None,
        joint_origin_scope_record_index: None,
    };
    alignment.operand_frames = exact_assembly_operand_frames(bytes, scope);
    alignment.operand_paths = alignment
        .operand_frames
        .as_ref()
        .and_then(|frames| exact_assembly_operand_paths(bytes, records, scope, frames));
    Some(alignment)
}

pub(crate) fn exact_component_insert_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignComponentInsertConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let relation_record_index = *scope.reference_members.first()?;
    if scope.kind != "Component Insert" || scope.reference_members.len() != 1 {
        return None;
    }
    let transform_at = match (scope.frame_length, scope.paired_class_tag.as_str()) {
        (399, "259")
            if bytes.get(start + 11..start + 20)? == [0; 9]
                && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                && bytes.get(start + 33..start + 37)? == [0; 4]
                && bytes.get(start + 37) == Some(&1)
                && u32_at(bytes, start + 38)? == relation_record_index
                && bytes.get(start + 42..start + 50)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
        {
            start + 50
        }
        (381, "261")
            if bytes.get(start + 11..start + 20)? == [0; 9]
                && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                && bytes.get(start + 33..start + 37)? == [0; 4]
                && bytes.get(start + 37) == Some(&1)
                && u32_at(bytes, start + 38)? == relation_record_index
                && bytes.get(start + 42..start + 49)? == [0, 0, 0, 0, 0, 0, 1] =>
        {
            start + 49
        }
        (395, "258")
            if bytes.get(start + 11..start + 21)? == [0; 10]
                && bytes.get(start + 29..start + 33)? == [0; 4]
                && bytes.get(start + 33) == Some(&1)
                && u32_at(bytes, start + 34)? == relation_record_index
                && bytes.get(start + 38..start + 46)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
        {
            start + 46
        }
        _ => return None,
    };
    let transform = rigid_transform_at(bytes, transform_at)?;
    let relation_at = records.first_at_or_after(0, relation_record_index)?;
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
    let carrier_at = unique_indexed_record_before(records, carrier_record_index, relation_at)?;
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
    if scope.frame_length == 381 {
        placements.extend(legacy_component_insert_placements(
            bytes,
            carrier_at,
            relation_at,
            carrier_record_index,
            transform,
        ));
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
        transform_offset: u64::try_from(transform_at).ok()?,
        carrier_transform_offset: u64::try_from(*carrier_transform_offset).ok()?,
    })
}

fn legacy_component_insert_placements(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    transform: [[f64; 4]; 4],
) -> Vec<(String, usize, usize)> {
    let Some((class_tag, after_tag)) =
        lp_ascii_filtered(bytes, carrier_at, 3..=3, u8::is_ascii_digit)
    else {
        return Vec::new();
    };
    if class_tag != "288"
        || after_tag != carrier_at + 7
        || u32_at(bytes, after_tag) != Some(carrier_record_index)
    {
        return Vec::new();
    }
    let mut placements = Vec::new();
    for first_at in carrier_at + 11..relation_at {
        let Some((first_guid, role_at)) = lp_utf16_bounded(bytes, first_at, 36..=36) else {
            continue;
        };
        let Some((role, after_role)) = lp_utf16_bounded(bytes, role_at, 36..=36) else {
            continue;
        };
        if !crate::bytes::is_guid_relaxed(&first_guid)
            || !crate::bytes::is_guid_relaxed(&role)
            || bytes.get(after_role..after_role + 14)
                != Some(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
        {
            continue;
        }
        let Some((asset_guid, after_asset_guid)) =
            lp_utf16_bounded(bytes, after_role + 14, 36..=36)
        else {
            continue;
        };
        let Some((asset_identity, after_asset_identity)) =
            lp_utf16_bounded(bytes, after_asset_guid + 1, 37..=256)
        else {
            continue;
        };
        if !crate::bytes::is_guid_relaxed(&asset_guid)
            || !asset_identity
                .split_once('_')
                .is_some_and(|(guid, locator)| {
                    crate::bytes::is_guid_relaxed(guid) && locator.starts_with("urn:")
                })
            || bytes.get(after_asset_guid) != Some(&0)
            || bytes.get(after_asset_identity) != Some(&0)
        {
            continue;
        }
        let carrier_transform_at = after_asset_identity + 1;
        let after_transform = carrier_transform_at + 16 * 8;
        let Some((repeated_identity, after_repeated_identity)) =
            lp_utf16_bounded(bytes, after_transform + 4, 37..=256)
        else {
            continue;
        };
        if rigid_transform_at(bytes, carrier_transform_at) == Some(transform)
            && repeated_identity == asset_identity
            && bytes.get(after_transform..after_transform + 4) == Some(&[0; 4])
            && bytes.get(after_repeated_identity..relation_at)
                == Some(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        {
            placements.push((role, role_at + 4, carrier_transform_at));
        }
    }
    placements
}

fn exact_copy_paste_component_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Option<DesignCopyPasteComponentOperation> {
    let stream = native_stream(&scope.id)?;
    let start = usize::try_from(scope.byte_offset).ok()?;
    let relation_record_index = *scope.reference_members.first()?;
    // The compact frame omits one four-byte prologue field, so both placements
    // and every marked reference before them move four bytes earlier.
    let source_at = match (scope.kind.as_str(), scope.frame_length) {
        ("CopyPaste", 529) => 38,
        ("CopyPaste", 525) => 34,
        _ => return None,
    };
    if scope.reference_members.len() != 1 {
        return None;
    }
    let source_transform = rigid_transform_at(bytes, start + source_at)?;
    let copied_transform = rigid_transform_at(bytes, start + source_at + 156)?;
    let relation_at = records.first_at_or_after(0, relation_record_index)?;
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

fn unique_indexed_record_before(
    records: &IndexedRecordOffsets,
    record_index: u32,
    end: usize,
) -> Option<usize> {
    let offsets = records.offsets(record_index);
    let [at] = &offsets[..offsets.partition_point(|offset| *offset < end)] else {
        return None;
    };
    Some(*at)
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
    enum FrameVariant {
        Standard,
        Compact,
        Axial,
    }

    let start = usize::try_from(scope.byte_offset).ok()?;
    let (frame_offsets, frame_variant) = match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length,
    ) {
        (_, "259", 637 | 692) | ("459", "264", 627) => ((28, 40, 168, 180), FrameVariant::Standard),
        (_, "258", 633 | 732) => ((24, 36, 164, 176), FrameVariant::Compact),
        (_, "261", 772) => ((28, 39, 167, 178), FrameVariant::Axial),
        _ => return None,
    };
    if usize::try_from(scope.paired_byte_offset).ok()?
        != start.checked_add(usize::try_from(scope.frame_length).ok()?)?
        || bytes.get(start + 11..start + 20)? != [0; 9]
    {
        return None;
    }
    if matches!(frame_variant, FrameVariant::Standard) {
        if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
            || !matches!(bytes.get(start + 25), Some(0 | 1))
            || bytes.get(start + 26..start + 28)? != [0; 2]
            || bytes.get(start + 33..start + 40)? != [0; 7]
            || bytes.get(start + 173..start + 180)? != [0; 7]
            || bytes.get(start + 308..start + 312)? != [0; 4]
        {
            return None;
        }
    } else if matches!(frame_variant, FrameVariant::Compact) {
        let compact_flags = bytes.get(start + 20..start + 24)?;
        if (compact_flags != [0; 4] && compact_flags != [0, 1, 0, 0])
            || bytes.get(start + 29..start + 36)? != [0; 7]
            || bytes.get(start + 169..start + 176)? != [0; 7]
            || bytes.get(start + 304..start + 308)? != [0; 4]
        {
            return None;
        }
    } else if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || !matches!(bytes.get(start + 25), Some(0 | 1))
        || bytes.get(start + 26..start + 28)? != [0; 2]
        || bytes.get(start + 33..start + 39)? != [0; 6]
        || bytes.get(start + 172..start + 178)? != [0; 6]
        || bytes.get(start + 306..start + 310)? != [0; 4]
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
    let first = frame(start + frame_offsets.0, start + frame_offsets.1)?;
    let second = frame(start + frame_offsets.2, start + frame_offsets.3)?;
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

fn exact_assembly_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Option<[DesignAssemblyOperandPath; 2]> {
    let search_start = usize::try_from(scope.paired_byte_offset).ok()?;
    let construction_at =
        records.first_at_or_after(search_start, frames[0].reference_record_index)?;
    let (first_delta, second_delta) = if scope.frame_length == 732 {
        (39, 36)
    } else {
        (5, 2)
    };
    let first_record_index = frames[0].reference_record_index.checked_sub(first_delta)?;
    let second_record_index = frames[0].reference_record_index.checked_sub(second_delta)?;
    let first_at = records.first_at_or_after(search_start, first_record_index)?;
    let second_at = records.first_at_or_after(first_at + 11, second_record_index)?;
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
    if read_u64(bytes, after_tag)? != u64::from(record_index)
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
    let mut identity_guids = Vec::new();
    let mut identity_guid_offsets = Vec::new();
    match class_tag.as_str() {
        "329" => {}
        "386" | "390" => {
            let end = next_indexed_record_offset(bytes, start + 1)?;
            if end > limit {
                return None;
            }
            for _ in 0..2 {
                let (guid, after_guid) = lp_utf16_bounded(bytes.get(..end)?, position, 36..=36)?;
                if !crate::bytes::is_guid_relaxed(&guid) {
                    return None;
                }
                identity_guid_offsets.push(u64::try_from(position + 4).ok()?);
                identity_guids.push(guid);
                position = after_guid;
            }
            if read_u64(bytes, position)? != 2 {
                return None;
            }
            position += 8;
            for _ in 0..2 {
                let (guid, after_guid) = lp_utf16_bounded(bytes.get(..end)?, position, 36..=36)?;
                if !crate::bytes::is_guid_relaxed(&guid) {
                    return None;
                }
                identity_guid_offsets.push(u64::try_from(position + 4).ok()?);
                identity_guids.push(guid);
                position = after_guid;
            }
            if u32_at(bytes, position)? != 2
                || !bytes.get(position + 4..end)?.iter().all(|byte| *byte == 0)
            {
                return None;
            }
        }
        _ => return None,
    }
    Some(DesignAssemblyOperandPath {
        record_index,
        class_tag,
        byte_offset: u64::try_from(start).ok()?,
        occurrence_guids,
        occurrence_guid_offsets,
        identity_guids,
        identity_guid_offsets,
    })
}

pub(crate) fn exact_rectangular_pattern_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
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
    construction.instances =
        exact_rectangular_pattern_instances(bytes, records, scope, &construction);
    Some(construction)
}

fn exact_rectangular_pattern_instances(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
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
            records
                .first_at_or_after(0, *record_index)
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
    records: &IndexedRecordOffsets,
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
        for (start, paired_at) in records.frames(*record_index) {
            if let Some((origin, direction)) = exact_circular_pattern_axis(
                bytes,
                start,
                paired_at,
                *record_index,
                *selection_record_index,
                scope.record_index,
            ) {
                axis_candidates.push((
                    crate::records::DesignCircularPatternAxis::Inline {
                        origin,
                        origin_offset: (start + 25) as u64,
                        direction,
                        direction_offset: (start + 49) as u64,
                    },
                    *record_index,
                    *selection_record_index,
                ));
            }
        }
    }
    for record_index in &scope.reference_members {
        for (start, paired_at) in records.frames(*record_index) {
            if let Some((axis, selection_record_index)) = exact_legacy_circular_pattern_axis(
                bytes,
                records,
                start,
                paired_at,
                *record_index,
                scope,
            ) {
                axis_candidates.push((axis, *record_index, selection_record_index));
            }
        }
    }
    let [(axis, record_index, selection_record_index)] = axis_candidates.as_slice() else {
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
            exact_fixed_pattern_count(bytes, records, *record_index, scope.record_index)
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
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
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
        axis: axis.clone(),
        axis_record_index: *record_index,
        selection_record_index: *selection_record_index,
    })
}

fn exact_legacy_circular_pattern_axis(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    start: usize,
    paired_at: usize,
    record_index: u32,
    scope: &DesignParameterScope,
) -> Option<(crate::records::DesignCircularPatternAxis, u32)> {
    use crate::records::DesignCircularPatternAxis;

    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != start + 7
        || u32_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
    {
        return None;
    }
    let (identity_offsets, selection_at, second_count_at, second_identity_at, scope_at, tail_at) =
        match (paired_at.checked_sub(start), u32_at(bytes, start + 21)) {
            (Some(129), Some(1)) => (
                vec![start + 26, start + 56],
                start + 40,
                start + 51,
                start + 55,
                start + 66,
                start + 77,
            ),
            (Some(118), Some(0)) => (
                vec![start + 45],
                start + 29,
                start + 40,
                start + 44,
                start + 55,
                start + 66,
            ),
            _ => return None,
        };
    let first_identity_at = identity_offsets.first().copied()?.checked_sub(1)?;
    if (identity_offsets.len() == 2
        && (marked_record_reference(bytes, first_identity_at).is_none()
            || bytes.get(first_identity_at + 5..first_identity_at + 11) != Some(&[0; 6])
            || u32_at(bytes, start + 36) != Some(1)))
        || u32_at(bytes, second_count_at) != Some(1)
        || marked_record_reference(bytes, second_identity_at).is_none()
        || bytes.get(second_identity_at + 5..second_identity_at + 11) != Some(&[0; 6])
        || marked_record_reference(bytes, scope_at) != Some(scope.record_index)
        || bytes.get(scope_at + 5..scope_at + 11) != Some(&[0; 6])
    {
        return None;
    }
    let selection_record_index = marked_record_reference(bytes, selection_at)?;
    if !scope.reference_members.contains(&selection_record_index)
        || bytes.get(selection_at + 5..selection_at + 11) != Some(&[0; 6])
    {
        return None;
    }
    let opaque_index = u32_at(bytes, tail_at)?;
    if opaque_index == 0
        || !f64_at(bytes, tail_at + 4)?.is_finite()
        || u32_at(bytes, tail_at + 12) != Some(opaque_index)
        || marked_record_reference(bytes, tail_at + 16) != record_index.checked_add(2)
        || bytes.get(tail_at + 21..tail_at + 27) != Some(&[0; 6])
        || bytes.get(tail_at + 27..tail_at + 29) != Some(&[0; 2])
        || marked_record_reference(bytes, tail_at + 29) != record_index.checked_add(1)
        || bytes.get(tail_at + 34..tail_at + 40) != Some(&[0; 6])
        || bytes.get(tail_at + 40) != Some(&0)
        || marked_record_reference(bytes, tail_at + 41) != Some(scope.record_index)
        || bytes.get(tail_at + 46..tail_at + 52) != Some(&[0; 6])
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
    let wrapper_record_indices = identity_offsets
        .iter()
        .map(|offset| u32_at(bytes, *offset))
        .collect::<Option<Vec<_>>>()?;
    let wrappers = wrapper_record_indices
        .iter()
        .map(|record_index| exact_pattern_identity_wrapper(bytes, records, *record_index))
        .collect::<Option<Vec<_>>>()?;
    let mut persistent_identities = wrappers
        .iter()
        .map(|(identity, _)| *identity)
        .collect::<Vec<_>>();
    persistent_identities.sort_unstable();
    persistent_identities.dedup();
    let [persistent_identity] = persistent_identities.as_slice() else {
        return None;
    };
    Some((
        DesignCircularPatternAxis::HistoricalEdge {
            wrapper_record_indices,
            persistent_identities: vec![*persistent_identity],
            identity_offsets: wrappers.into_iter().map(|(_, offset)| offset).collect(),
            resolved_origin: None,
            resolved_direction: None,
        },
        selection_record_index,
    ))
}

fn exact_pattern_identity_wrapper(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<(u64, u64)> {
    let [start] = records.offsets(record_index) else {
        return None;
    };
    let start = *start;
    let (_, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start + 7
        || u32_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || read_u64(bytes, start + 21)? == 0
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 29, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !crate::bytes::is_guid_relaxed(&asset_id)
        || !crate::bytes::is_guid_relaxed(&context_id)
        || u32_at(bytes, after_context_id) != Some(2)
        || bytes.get(after_context_id + 4..after_context_id + 8) != Some(&[0; 4])
        || marked_record_reference(bytes, after_context_id + 8) != record_index.checked_add(1)
        || bytes.get(after_context_id + 13..after_context_id + 19) != Some(&[0; 6])
    {
        return None;
    }
    let nested_one_at = next_indexed_record_offset(bytes, after_context_id + 19)?;
    let (_, nested_one_tag) = lp_ascii_filtered(bytes, nested_one_at, 3..=3, u8::is_ascii_digit)?;
    if u32_at(bytes, nested_one_tag) != record_index.checked_add(1)
        || bytes.get(nested_one_at + 11..nested_one_at + 21) != Some(&[0; 10])
        || marked_record_reference(bytes, nested_one_at + 21) != record_index.checked_add(2)
        || bytes.get(nested_one_at + 26..nested_one_at + 32) != Some(&[0; 6])
    {
        return None;
    }
    let identity_at = next_indexed_record_offset(bytes, nested_one_at + 32)?;
    let (_, identity_tag) = lp_ascii_filtered(bytes, identity_at, 3..=3, u8::is_ascii_digit)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 29)?;
    let (_, next_tag) = lp_ascii_filtered(bytes, next_at, 3..=3, u8::is_ascii_digit)?;
    if u32_at(bytes, identity_tag) != record_index.checked_add(2)
        || bytes.get(identity_at + 11..identity_at + 21) != Some(&[0; 10])
        || identity_at.checked_add(29) != Some(next_at)
        || u32_at(bytes, next_tag) != record_index.checked_add(3)
    {
        return None;
    }
    Some((
        read_u64(bytes, identity_at + 21)?,
        u64::try_from(identity_at + 21).ok()?,
    ))
}

/// Join a Mirror scope's two operand groups and fixed parameters with either a
/// referenced `WorkPlane` or a persistent plane-face selection.
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
            .filter(|group| matches!(group.role, 0x0000_0004_0000_0000 | 0x0000_0008_0000_0000))
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
        let work_plane = compact_feature_reference(bytes, plane_header).and_then(
            |(plane_reference, plane_reference_offset)| {
                plane_reference
                    .checked_add(1)
                    .filter(|record_index| {
                        scopes.iter().any(|scope| {
                            native_stream(&scope.id) == Some(stream)
                                && scope.record_index == *record_index
                                && scope.kind == "WorkPlane"
                                && scope.work_plane_transform.is_some()
                        })
                    })
                    .map(|record_index| (record_index, plane_reference_offset))
            },
        );
        let (plane_scope_record_index, plane_reference_offset, plane_selection_record_index) =
            if let Some((plane_scope_record_index, plane_reference_offset)) = work_plane {
                (
                    Some(plane_scope_record_index),
                    Some(plane_reference_offset),
                    None,
                )
            } else if crate::design::decode::operands::parse_entity_selection_operand(
                bytes,
                plane_group,
                0,
                plane_header,
            )
            .is_some()
            {
                (None, None, Some(*plane_member))
            } else {
                continue;
            };
        let seed_feature = match seed_group.members.as_slice() {
            _ if seed_group.role != 0x0000_0008_0000_0000 => None,
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
            plane_selection_record_index,
            plane_origin: None,
            plane_normal: None,
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
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<(u32, u64)> {
    let candidates = records
        .frames(record_index)
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
    records: &IndexedRecordOffsets,
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
    let body_group_at = records.first_at_or_after(search_at, body_group_record_index)?;
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
    let relation_at = records.first_at_or_after(search_at, relation_record_index)?;
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
            metadata_field: bytes.get(start + 45..start + 51)?.to_vec(),
            result_records: Vec::new(),
            result_record_offsets: Vec::new(),
            result_fields: Vec::new(),
        });
    }
    if bytes.get(start + 19) != Some(&1) {
        return None;
    }
    let combined_count = usize::try_from(u32_at(bytes, start + 20)?).ok()?;
    if combined_count == 0 || combined_count > 200_000 || combined_count % 2 != 0 {
        return None;
    }
    let body_count = combined_count / 2;
    let expanded = scope.class_tag == "384" && scope.paired_class_tag == "264";
    let legacy_compact = scope.class_tag == "420" && scope.paired_class_tag == "258";
    let base_length = if expanded || legacy_compact { 262 } else { 271 };
    if scope.frame_length != base_length + u64::try_from(body_count.checked_mul(52)?).ok()? {
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
    if expanded {
        if bytes.get(cursor) != Some(&1)
            || bytes.get(cursor + 1..cursor + 7) != Some(&[0; 6])
            || usize::try_from(u32_at(bytes, cursor + 7)?).ok()? != body_count
        {
            return None;
        }
        cursor += 11;
    } else if legacy_compact {
        if bytes.get(cursor) != Some(&1)
            || bytes.get(cursor + 1..cursor + 6) != Some(&[0; 5])
            || bytes.get(cursor + 6) != Some(&1)
            || usize::try_from(u32_at(bytes, cursor + 7)?).ok()? != body_count
        {
            return None;
        }
        cursor += 11;
    } else {
        if bytes.get(cursor) != Some(&1) || bytes.get(cursor + 1..cursor + 11) != Some(&[0; 10]) {
            return None;
        }
        cursor += 11;
        if usize::try_from(u32_at(bytes, cursor)?).ok()? != body_count {
            return None;
        }
        cursor += 4;
    }
    let mut repeated_reference_fields = Vec::with_capacity(body_count);
    for ordinal in 0..body_count {
        let expected = if legacy_compact {
            u32::try_from(body_entity_suffixes[ordinal]).ok()?
        } else {
            body_reference_records[ordinal]
        };
        if bytes.get(cursor) != Some(&1) || u32_at(bytes, cursor + 1)? != expected {
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
    let metadata_field_width = if expanded || legacy_compact { 2 } else { 6 };
    let metadata_field = bytes
        .get(cursor + 9..cursor + 9 + metadata_field_width)?
        .to_vec();
    cursor += 9 + metadata_field_width;
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
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignSolidPrimitive> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let (operation, operation_offset) = match scope.kind.as_str() {
        "SpherePrimitive" | "TorusPrimitive" => {
            let operation_offset = start.checked_add(25)?;
            (
                primitive_operation(bytes, operation_offset)?,
                operation_offset,
            )
        }
        "BoxPrimitive" | "CylinderPrimitive" => {
            if bytes.get(start + 11..start + 20)? != [0; 9]
                || bytes.get(start + 24) != Some(&0)
                || bytes.get(start + 25) != Some(&1)
            {
                return None;
            }
            let operation_offset = start.checked_add(20)?;
            (
                primitive_operation(bytes, operation_offset)?,
                operation_offset,
            )
        }
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
                exact_primitive_diameter(bytes, records, diameter_record_index)?;
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
                exact_primitive_diameter(bytes, records, major_diameter_record_index)?;
            let (minor_diameter, minor_diameter_offset) =
                exact_primitive_diameter(bytes, records, minor_diameter_record_index)?;
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
        "BoxPrimitive" => {
            if scope.frame_length < 78 || scope.reference_members.len() < 5 {
                return None;
            }
            let owners = exact_owned_primitive_parameters(scope, parameter_owners, 5)?;
            let [length, width, height, offset_x, offset_y] = owners.as_slice() else {
                return None;
            };
            (length.evaluated_value > 0.0
                && width.evaluated_value > 0.0
                && height.evaluated_value > 0.0)
                .then_some(DesignSolidPrimitive::Box {
                    length: length.evaluated_value,
                    length_record_index: length.record_index,
                    length_offset: length.evaluated_value_offset,
                    width: width.evaluated_value,
                    width_record_index: width.record_index,
                    width_offset: width.evaluated_value_offset,
                    height: height.evaluated_value,
                    height_record_index: height.record_index,
                    height_offset: height.evaluated_value_offset,
                    offset_x: offset_x.evaluated_value,
                    offset_x_record_index: offset_x.record_index,
                    offset_x_offset: offset_x.evaluated_value_offset,
                    offset_y: offset_y.evaluated_value,
                    offset_y_record_index: offset_y.record_index,
                    offset_y_offset: offset_y.evaluated_value_offset,
                    operation,
                    operation_offset: operation_offset as u64,
                })
        }
        "CylinderPrimitive" => {
            if scope.frame_length < 78 || scope.reference_members.len() < 2 {
                return None;
            }
            let owners = exact_owned_primitive_parameters(scope, parameter_owners, 2)?;
            let [height, diameter] = owners.as_slice() else {
                return None;
            };
            (height.evaluated_value > 0.0 && diameter.evaluated_value > 0.0).then_some(
                DesignSolidPrimitive::Cylinder {
                    height: height.evaluated_value,
                    height_record_index: height.record_index,
                    height_offset: height.evaluated_value_offset,
                    diameter: diameter.evaluated_value,
                    diameter_record_index: diameter.record_index,
                    diameter_offset: diameter.evaluated_value_offset,
                    operation,
                    operation_offset: operation_offset as u64,
                },
            )
        }
        _ => None,
    }
}

fn primitive_operation(bytes: &[u8], offset: usize) -> Option<DesignExtrudeOperation> {
    match u32_at(bytes, offset)? {
        1 => Some(DesignExtrudeOperation::Join),
        2 => Some(DesignExtrudeOperation::Cut),
        3 => Some(DesignExtrudeOperation::Intersect),
        4 => Some(DesignExtrudeOperation::NewBody),
        _ => None,
    }
}

fn exact_owned_primitive_parameters<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    count: usize,
) -> Option<Vec<&'a DesignParameterOwner>> {
    let stream = native_stream(&scope.id)?;
    let mut owners = parameter_owners
        .iter()
        .filter(|owner| {
            owner.scope_record_index == scope.record_index
                && native_stream(&owner.id) == Some(stream)
                && scope.reference_members.contains(&owner.record_index)
                && owner.evaluated_value.is_finite()
        })
        .collect::<Vec<_>>();
    owners.sort_by_key(|owner| owner.local_ordinal);
    if owners.len() != count
        || owners
            .windows(2)
            .any(|pair| pair[0].local_ordinal == pair[1].local_ordinal)
        || owners
            .iter()
            .enumerate()
            .any(|(ordinal, owner)| owner.local_ordinal != ordinal as u32)
    {
        return None;
    }
    Some(owners)
}

fn exact_primitive_diameter(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<(f64, u64)> {
    let scalar = exact_fixed_scalar(bytes, records, record_index)?;
    (scalar.value > 0.0).then_some((scalar.value, scalar.value_offset))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FixedScalarFrame {
    owner_record_index: Option<u32>,
    ordinal: u8,
    value: f64,
    value_offset: u64,
}

fn exact_fixed_scalar(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<FixedScalarFrame> {
    let candidates = records
        .frames(record_index)
        .filter_map(|(start, end)| {
            let frame_length = end.checked_sub(start)?;
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
    records: &IndexedRecordOffsets,
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
            let scalar = exact_fixed_scalar(bytes, records, distance_record_index)?;
            Some(DesignDirectFaceOperation::OffsetFaces {
                distance: scalar.value,
                distance_record_index,
                distance_offset: scalar.value_offset,
            })
        }
        DesignFeatureFamily::Thicken if scope.reference_members.len() >= 3 => {
            let (reference_offset, thickness_is_first) = match parameter_scope_payload_length(scope)
            {
                Some(length)
                    if length
                        == 276
                            + 11 * u64::try_from(scope.reference_members.len().checked_sub(2)?)
                                .ok()?
                        && bytes.get(start + 34) == Some(&1)
                        && u32_at(bytes, start + 35) == scope.reference_members.get(1).copied()
                        && bytes.get(start + 39..start + 45) == Some(&[0; 6])
                        && matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46..start + 48) == Some(&[1, 1])
                        && u32_at(bytes, start + 48)
                            == scope.reference_members.first().copied() =>
                {
                    (47, true)
                }
                Some(281)
                    if matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46) == Some(&1) =>
                {
                    (46, false)
                }
                Some(287) if bytes.get(start + 47) == Some(&1) => (47, false),
                _ => return None,
            };
            let thickness_record_index = u32_at(bytes, start + reference_offset + 1)?;
            let expected_thickness = if thickness_is_first {
                scope.reference_members.first()
            } else {
                scope.reference_members.last()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
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
            let (thickness_record_index, thickness_is_first, outward, outward_offset) =
                match parameter_scope_payload_length(scope) {
                    Some(268)
                        if bytes.get(start + 11..start + 20) == Some(&[0; 9])
                            && bytes.get(start + 20) == Some(&1)
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22..start + 25) == Some(&[0; 3])
                            && bytes.get(start + 25..start + 27) == Some(&[1, 0])
                            && bytes.get(start + 27) == Some(&1)
                            && bytes.get(start + 32..start + 51) == Some(&[0; 19])
                            && u32_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && u32_at(bytes, start + 56)
                                == scope.reference_members.get(1).copied()
                            && bytes.get(start + 60..start + 66) == Some(&[0; 6]) =>
                    {
                        (
                            u32_at(bytes, start + 28)?,
                            true,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
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
                            false,
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
                            false,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    _ => return None,
                };
            let expected_thickness = if thickness_is_first {
                scope.reference_members.first()
            } else {
                scope.reference_members.last()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
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
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignMoveOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Move) {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in records.frames(*record_index) {
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
    records: &IndexedRecordOffsets,
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
    let fixed_lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    let embedded_distances = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            exact_embedded_extrude_distance(bytes, records, *record_index, scope.record_index)
                .map(|scalar| (*record_index, scalar))
        })
        .collect::<Vec<_>>();
    if fixed_lanes.len() > 2 || embedded_distances.len() > 1 {
        return None;
    }
    let mut along_distance = embedded_distances.first().map(|(record_index, lane)| {
        DesignFixedExtrudeDistance::DistanceConstruction(DesignFixedExtrudeScalar {
            value: lane.value,
            record_index: *record_index,
            value_offset: lane.value_offset,
        })
    });
    let mut taper_angle = None;
    let mut seen_fixed_ordinals = [false; 2];
    for (record_index, lane) in fixed_lanes {
        let ordinal = usize::from(lane.ordinal);
        if ordinal >= seen_fixed_ordinals.len() || seen_fixed_ordinals[ordinal] {
            return None;
        }
        seen_fixed_ordinals[ordinal] = true;
        let scalar = DesignFixedExtrudeScalar {
            value: lane.value,
            record_index,
            value_offset: lane.value_offset,
        };
        match lane.ordinal {
            0 if lane.value != 0.0 && along_distance.is_none() => {
                along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(scalar));
            }
            0 if along_distance.is_some() && lane.value == 0.0 => {}
            1 if taper_angle.is_none() => taper_angle = Some(scalar),
            _ => return None,
        }
    }
    if along_distance.is_none() && taper_angle.is_none() {
        return None;
    }
    Some(DesignFixedExtrudeParameters {
        along_distance,
        taper_angle,
    })
}

fn exact_embedded_extrude_distance(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<FixedScalarFrame> {
    let candidates = records
        .frames(record_index)
        .filter_map(|(start, end)| {
            (end.checked_sub(start)? == 100).then_some(())?;
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            let first_auxiliary = record_index.checked_add(1)?;
            let second_auxiliary = record_index.checked_add(2)?;
            if after_tag != start + 7
                || class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || bytes.get(start + 11..start + 21) != Some(&[0; 10])
                || marked_record_reference(bytes, start + 21)? != scope_record_index
                || bytes.get(start + 26..start + 32) != Some(&[0; 6])
                || u32_at(bytes, start + 32)? != 1
                || marked_record_reference(bytes, start + 36).is_none()
                || bytes.get(start + 41..start + 47) != Some(&[0; 6])
                || u32_at(bytes, start + 47)? != 210
                || u32_at(bytes, start + 59)? != 210
                || marked_record_reference(bytes, start + 63)? != second_auxiliary
                || bytes.get(start + 68..start + 74) != Some(&[0; 6])
                || bytes.get(start + 74..start + 77) != Some(&[1, 0, 0])
                || marked_record_reference(bytes, start + 77)? != first_auxiliary
                || bytes.get(start + 82..start + 89) != Some(&[0; 7])
                || marked_record_reference(bytes, start + 89)? != scope_record_index
                || bytes.get(start + 94..start + 100) != Some(&[0; 6])
            {
                return None;
            }
            let value = f64_at(bytes, start + 51)?;
            (value.is_finite() && value > 0.0).then_some(FixedScalarFrame {
                owner_record_index: Some(scope_record_index),
                ordinal: 0,
                value,
                value_offset: u64::try_from(start + 51).ok()?,
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(*candidate)
}

pub(crate) fn exact_fixed_fillet_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignFixedFilletParameters> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Fillet) {
        return None;
    }
    let lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
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
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFixedChamferParameters> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Chamfer) {
        return None;
    }
    let stream = native_stream(&scope.id);
    if parameter_owners.iter().any(|owner| {
        stream.is_some()
            && native_stream(&owner.id) == stream
            && owner.scope_record_index == scope.record_index
    }) {
        return None;
    }
    let lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
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
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
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
            if scope.class_tag == "409"
                && scope.paired_class_tag == "257"
                && parameter_scope_payload_length(scope) == Some(345)
                && scope.reference_members.len() == 6
                && bytes.get(start + 20) == Some(&1)
                && u32_at(bytes, start + 21) == Some(0)
                && u32_at(bytes, start + 29) == Some(2)
                && bytes.get(start + 33) == Some(&0)
                && u32_at(bytes, start + 34) == Some(1) =>
        {
            let angle_record_index = *scope.reference_members.get(4)?;
            let candidates = parameter_owners
                .iter()
                .filter(|owner| {
                    native_stream(&owner.id) == native_stream(&scope.id)
                        && owner.scope_record_index == scope.record_index
                        && owner.record_index == angle_record_index
                        && owner.local_ordinal == 0
                        && owner.evaluated_value.is_finite()
                        && owner.evaluated_value > 0.0
                })
                .collect::<Vec<_>>();
            let [angle] = candidates.as_slice() else {
                return None;
            };
            Some(DesignPathFeatureConstruction::Revolve {
                operation: operation(start + 25)?,
                operation_offset: u64::try_from(start + 25).ok()?,
                angle: angle.evaluated_value,
                angle_record_index,
                angle_offset: angle.evaluated_value_offset,
                opposite_angle_record_index: None,
                opposite_angle_offset: None,
            })
        }
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
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
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
                opposite_angle_record_index: Some(*opposite_angle_record_index),
                opposite_angle_offset: Some(opposite.value_offset),
            })
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag == "407"
                && scope.paired_class_tag == "258"
                && parameter_scope_payload_length(scope) == Some(363)
                && scope.reference_members.len() == 8
                && u32_at(bytes, start + 25) == Some(2)
                && bytes.get(start + 29) == Some(&0)
                && u32_at(bytes, start + 30) == Some(1)
                && bytes.get(start + 34) == Some(&1)
                && bytes.get(start + 43..start + 45) == Some(&[0; 2]) =>
        {
            let angle_record_index = u32::try_from(read_u64(bytes, start + 35)?).ok()?;
            if scope.reference_members.get(6) != Some(&angle_record_index) {
                return None;
            }
            let candidates = parameter_owners
                .iter()
                .filter(|owner| {
                    native_stream(&owner.id) == native_stream(&scope.id)
                        && owner.scope_record_index == scope.record_index
                        && owner.record_index == angle_record_index
                        && owner.local_ordinal == 0
                        && owner.evaluated_value.is_finite()
                        && owner.evaluated_value > 0.0
                })
                .collect::<Vec<_>>();
            let [angle] = candidates.as_slice() else {
                return None;
            };
            Some(DesignPathFeatureConstruction::Revolve {
                operation: operation(start + 21)?,
                operation_offset: u64::try_from(start + 21).ok()?,
                angle: angle.evaluated_value,
                angle_record_index,
                angle_offset: angle.evaluated_value_offset,
                opposite_angle_record_index: None,
                opposite_angle_offset: None,
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
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
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
        DesignFeatureFamily::Pipe => {
            let lanes = scope
                .reference_members
                .iter()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index))
                        .then_some((*record_index, scalar))
                })
                .collect::<Vec<_>>();
            let lanes: [(u32, FixedScalarFrame); 4] = lanes.try_into().ok()?;
            if lanes
                .iter()
                .enumerate()
                .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
            {
                return None;
            }
            let section_shape = *bytes.get(start + 29)?;
            let filled = match *bytes.get(start + 30)? {
                0 => false,
                1 => true,
                _ => return None,
            };
            Some(DesignPathFeatureConstruction::Pipe {
                operation: operation(start + 25)?,
                operation_offset: u64::try_from(start + 25).ok()?,
                section_shape,
                section_shape_offset: u64::try_from(start + 29).ok()?,
                filled,
                filled_offset: u64::try_from(start + 30).ok()?,
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
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in records.frames(*record_index) {
            let frame_length = paired.checked_sub(start)?;
            let (matrix_at, reference) = match frame_length {
                321 if bytes.get(start + 11..start + 49) == Some(&[0u8; 38][..]) => {
                    (start + 49, None)
                }
                325 if bytes.get(start + 4..start + 7) == Some(b"431")
                    && bytes.get(paired + 4..paired + 7) == Some(b"257")
                    && bytes.get(start + 11..start + 49) == Some(&[0u8; 38][..]) =>
                {
                    (start + 49, None)
                }
                321 if bytes.get(start + 4..start + 7) == Some(b"364")
                    && bytes.get(paired + 4..paired + 7) == Some(b"264")
                    && bytes.get(start + 11..start + 46) == Some(&[0u8; 35][..])
                    && bytes.get(start + 46..start + 49) == Some(&[1, 0, 0][..]) =>
                {
                    (start + 49, None)
                }
                321 if bytes.get(start + 4..start + 7) == Some(b"364")
                    && bytes.get(paired + 4..paired + 7) == Some(b"264")
                    && bytes.get(start + 11..start + 45) == Some(&[0u8; 34][..])
                    && bytes.get(start + 45..start + 49) == Some(&[0xcc, 0xcd, 0, 0][..]) =>
                {
                    (start + 49, None)
                }
                326 if matches!(
                    (
                        bytes.get(start + 4..start + 7),
                        bytes.get(paired + 4..paired + 7),
                    ),
                    (Some(b"409"), Some(b"258")) | (Some(b"450"), Some(b"259"))
                ) && bytes.get(start + 11..start + 50) == Some(&[0u8; 39][..]) =>
                {
                    (start + 50, None)
                }
                337 if bytes.get(start + 4..start + 7) == Some(b"409")
                    && bytes.get(paired + 4..paired + 7) == Some(b"258")
                    && bytes.get(start + 11..start + 50) == Some(&[0u8; 39][..]) =>
                {
                    (start + 50, None)
                }
                352 | 363 | 374
                    if bytes.get(start + 55) == Some(&1)
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

pub(crate) fn exact_work_axis_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignWorkAxisConstruction> {
    if scope.kind != "WorkAxis" {
        return None;
    }
    let [axis_record_index, _, first_point_record_index, _, second_point_record_index] =
        scope.reference_members.as_slice()
    else {
        return None;
    };
    let axis_frames = records.frames(*axis_record_index).collect::<Vec<_>>();
    let [(axis_start, axis_paired)] = axis_frames.as_slice() else {
        return None;
    };
    if axis_paired.checked_sub(*axis_start)? != 232
        || bytes.get(axis_start + 11..axis_start + 21) != Some(&[0; 10])
        || u32_at(bytes, axis_start + 21)? != 8
        || u32_at(bytes, axis_start + 118)? != 2
    {
        return None;
    }
    let values = f64s_at(bytes, axis_start + 25, 8)?;
    if values.iter().any(|value| !value.is_finite()) || values[6..] != [0.0, 0.0] {
        return None;
    }
    let origin: [f64; 3] = values[..3].try_into().ok()?;
    let displacement: [f64; 3] = values[3..6].try_into().ok()?;
    let displacement_length = displacement[0]
        .hypot(displacement[1])
        .hypot(displacement[2]);
    if displacement_length <= f64::EPSILON {
        return None;
    }
    let point_record_indices = [*first_point_record_index, *second_point_record_index];
    for (ordinal, expected) in point_record_indices.iter().enumerate() {
        let reference_at = axis_start + 122 + ordinal * 11;
        if bytes.get(reference_at) != Some(&1)
            || u32_at(bytes, reference_at + 1)? != *expected
            || bytes.get(reference_at + 5..reference_at + 11) != Some(&[0; 6])
        {
            return None;
        }
    }
    let mut points = [[0.0; 3]; 2];
    let mut point_offsets = [0; 2];
    for (ordinal, record_index) in point_record_indices.iter().enumerate() {
        let point_frames = records.frames(*record_index).collect::<Vec<_>>();
        let [(start, paired)] = point_frames.as_slice() else {
            return None;
        };
        if paired.checked_sub(*start)? != 197 || bytes.get(start + 11..start + 42) != Some(&[0; 31])
        {
            return None;
        }
        let point = f64s_at(bytes, start + 42, 3)?;
        if point.iter().any(|value| !value.is_finite()) {
            return None;
        }
        points[ordinal] = point.try_into().ok()?;
        point_offsets[ordinal] = u64::try_from(start + 42).ok()?;
    }
    let endpoint = std::array::from_fn(|axis| origin[axis] + displacement[axis]);
    if points != [origin, endpoint] {
        return None;
    }
    Some(DesignWorkAxisConstruction {
        origin,
        displacement,
        origin_offset: u64::try_from(axis_start + 25).ok()?,
        displacement_offset: u64::try_from(axis_start + 49).ok()?,
        point_record_indices,
        point_offsets,
    })
}

pub(crate) fn exact_joint_origin_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    if scope.kind != "JointOrigin" {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in records.frames(*record_index) {
            if paired.checked_sub(start)? == 385
                && bytes.get(start + 4..start + 7) == Some(b"364")
                && bytes.get(paired + 4..paired + 7) == Some(b"264")
                && bytes.get(start + 11..start + 45) == Some(&[0; 34])
                && bytes.get(start + 45..start + 49) == Some(&[1, 1, 0, 0])
            {
                let values = f64s_at(bytes, start + 49, 16)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.into_iter().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if valid_sketch_transform(&transform) {
                    candidates.push(ScopePlacementFrame {
                        transform,
                        transform_offset: (start + 49) as u64,
                        reference: None,
                    });
                }
                continue;
            }
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

/// Skip the payload prologue at `at`: a leading-block presence byte, a property
/// presence byte, and the property block that byte gates. The leading-block
/// byte belongs to classes that write one, so this reader only steps over it.
pub(crate) fn payload_prologue(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    let mut cursor = at.checked_add(1)?;
    let present = *bytes.get(cursor)?;
    cursor += 1;
    match present {
        0 => Some(cursor),
        1 => {
            let count = u32_at(bytes, cursor)?;
            if count > 16 {
                return None;
            }
            cursor += 4;
            for _ in 0..count {
                let (_key, after_key) =
                    lp_ascii_filtered(bytes, cursor, 1..=64, u8::is_ascii_graphic)?;
                let (type_name, after_type) =
                    lp_ascii_filtered(bytes, after_key, 1..=64, u8::is_ascii_graphic)?;
                if type_name != "IntrinsicMetaTypeuint64" {
                    return None;
                }
                cursor = after_type.checked_add(8)?;
            }
            (cursor <= end).then_some(cursor)
        }
        _ => None,
    }
}

/// Type GUID of the point-data class a `WorkPoint` scope references. Every
/// record of this class carries the `point3d` member sequence below.
const POINT_DATA_TYPE_GUID: &str = "69EE2FA7-BCC7-449E-9CA9-976CEFDFED44";

/// The base class level of a point-data record, read under one record version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PointDataLevel {
    /// Byte offset of `point3d`'s first coordinate.
    position_at: usize,
    /// Construction rule that produced the point.
    reference_type: u32,
    /// Record indices of the counted reference run that closes the level.
    input_record_indices: Vec<u32>,
}

/// Read the base class level of the point-data class at `start` under one
/// record version.
///
/// The level closes with a counted reference run. The serialized count is the
/// only framing authority for that run: `reference_type` selects the
/// construction rule, but its rule-specific arity is not encoded in the class
/// member order. The count is bounded by the frame before allocation and each
/// marked reference must resolve to a record index.
fn point_data_level(
    bytes: &[u8],
    start: usize,
    end: usize,
    version: u32,
) -> Option<PointDataLevel> {
    let body = bytes.get(..end)?;
    let mut cursor = payload_prologue(bytes, start, end)?;
    if version >= 2 {
        cursor = cursor.checked_add(4)?;
    }
    cursor = cursor.checked_add(16)?;
    if version >= 1 {
        take_reference(body, &mut cursor)?;
    }
    let position_at = cursor;
    cursor = cursor.checked_add(24)?;
    let reference_type = u32_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    if version >= 3 {
        cursor = cursor.checked_add(24)?;
    }
    let arity = usize::try_from(u32_at(body, cursor)?).ok()?;
    cursor = cursor.checked_add(4)?;
    if arity == 0 || arity > end.checked_sub(cursor)? {
        return None;
    }
    let mut input_record_indices = Vec::with_capacity(arity);
    for _ in 0..arity {
        let reference = take_reference(body, &mut cursor)?;
        input_record_indices.push(u32::try_from(reference.target?).ok()?);
    }
    Some(PointDataLevel {
        position_at,
        reference_type,
        input_record_indices,
    })
}

/// The base class level of the point-data record a `WorkPoint` scope selects.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorkPointFrame {
    /// Model-space coordinate.
    pub(crate) position: [f64; 3],
    /// Byte offset of the coordinate's first f64.
    pub(crate) position_offset: u64,
    /// Construction rule that produced the point.
    pub(crate) reference_type: u32,
    /// Record indices of the counted reference run that closes the level.
    pub(crate) input_record_indices: Vec<u32>,
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
pub(crate) fn exact_work_point_position(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<WorkPointFrame> {
    if scope.kind != "WorkPoint" {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in &scope.reference_members {
        for (start, paired) in records.frames(*record_index) {
            let Some((_class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if u32_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            // A class tag is `256` plus an index into the segment's own type
            // table, so it names a different class in every segment and cannot
            // select the point-data class. The type GUID can.
            let stored = stream_types.get(&u64::from(*record_index)).copied();
            if stored.is_some_and(|(type_guid, _)| type_guid != POINT_DATA_TYPE_GUID) {
                continue;
            }
            // The payload begins after the record name that closes the header.
            let Some((_name, payload_at)) =
                lp_ascii_filtered(bytes, after_tag + 8, 0..=256, u8::is_ascii_graphic)
            else {
                continue;
            };
            let mut levels = stored
                .map_or_else(|| (0..=3).collect::<Vec<_>>(), |(_, version)| vec![version])
                .into_iter()
                .filter_map(|version| point_data_level(bytes, payload_at, paired, version))
                .collect::<Vec<_>>();
            // Agreement is over the levels the fitting versions name, so the
            // duplicates are removed by value and not only where adjacent.
            levels.sort_unstable();
            levels.dedup();
            let [level] = levels.as_slice() else {
                continue;
            };
            let Some(position) = f64s_at(bytes, level.position_at, 3) else {
                continue;
            };
            let Ok(position): Result<[f64; 3], _> = position.try_into() else {
                continue;
            };
            if position.iter().all(|value| value.is_finite()) {
                candidates.push(WorkPointFrame {
                    position,
                    position_offset: level.position_at as u64,
                    reference_type: level.reference_type,
                    input_record_indices: level.input_record_indices.clone(),
                });
            }
        }
    }
    if candidates.len() != 1 {
        return None;
    }
    candidates.pop()
}

pub(crate) fn exact_combine_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignCombineOperation> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Combine)
        || scope.reference_members.len() < 4
        || !scope.reference_members.len().is_multiple_of(2)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let compact = scope.class_tag == "387"
        && scope.paired_class_tag == "258"
        && parameter_scope_payload_length(scope) == Some(314);
    let operation_offset = if compact {
        if bytes.get(start + 11..start + 21)? != [0; 10]
            || bytes.get(start + 26..start + 29)? != [0; 3]
            || bytes.get(start + 29..start + 31)? != [1, 0]
            || u32_at(bytes, start + 31) != Some(1)
            || bytes.get(start + 35) != Some(&1)
            || read_u64(bytes, start + 36) == Some(0)
            || bytes.get(start + 43..start + 45)? != [0; 2]
        {
            return None;
        }
        start + 21
    } else {
        if bytes.get(start + 11..start + 19)? != [0; 8]
            || bytes.get(start + 24) != Some(&0)
            || bytes.get(start + 26..start + 33)? != [0; 7]
        {
            return None;
        }
        start + 20
    };
    let operation = match u32_at(bytes, operation_offset)? {
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
        let [operation_at, operation_end] = records.offsets(*operation_record_index) else {
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
        let [selection_at, selection_end] = records.offsets(*selection_record_index) else {
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
        operation_offset: u64::try_from(operation_offset).ok()?,
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

pub(crate) fn exact_draft_operation_with_owners(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignDraftOperation> {
    // The frame is variable-length and carries six or more references, so no
    // frame length or reference count identifies the record. The ordered
    // reference table is in record-index order, so the two scalar lanes hold no
    // fixed position in it either: they sort before the operand groups in one
    // document and after them in another. The lanes are identified by their own
    // properties instead. They are the only scope-owned fixed scalars among the
    // references, and their local ordinals order them.
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Draft)
        || scope.reference_members.len() < 6
    {
        return None;
    }
    let scope_stream = native_stream(&scope.id);
    let mut lanes = scope
        .reference_members
        .iter()
        .filter_map(|record_index| {
            if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index) {
                return (scalar.owner_record_index == Some(scope.record_index)).then_some((
                    *record_index,
                    u32::from(scalar.ordinal),
                    scalar.value,
                    scalar.value_offset,
                ));
            }
            let owners = parameter_owners
                .iter()
                .filter(|owner| {
                    owner.record_index == *record_index
                        && owner.scope_record_index == scope.record_index
                        && scope_stream
                            .is_none_or(|stream| native_stream(&owner.id) == Some(stream))
                        && owner.evaluated_value.is_finite()
                })
                .collect::<Vec<_>>();
            let [owner] = owners.as_slice() else {
                return None;
            };
            Some((
                *record_index,
                owner.local_ordinal,
                owner.evaluated_value,
                owner.evaluated_value_offset,
            ))
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(|(_, ordinal, _, _)| *ordinal);
    let [(angle_record_index, angle_ordinal, angle, angle_offset), (opposite_angle_record_index, opposite_ordinal, opposite, opposite_offset)] =
        lanes.as_slice()
    else {
        return None;
    };
    if *angle_ordinal != 0 || *opposite_ordinal != 1 || !angle.is_finite() || *opposite != 0.0 {
        return None;
    }
    Some(DesignDraftOperation {
        angle: *angle,
        angle_record_index: *angle_record_index,
        angle_offset: *angle_offset,
        opposite_angle_record_index: *opposite_angle_record_index,
        opposite_angle_offset: *opposite_offset,
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

/// Every indexed-record header that can open a parameter scope: a scope is
/// delimited by two headers carrying its record index, so the last header of an
/// index opens nothing.
pub(crate) fn parameter_scope_candidate_headers(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
) -> Vec<DesignRecordHeader> {
    records
        .records()
        .flat_map(|(record_index, offsets)| {
            offsets[..offsets.len().saturating_sub(1)]
                .iter()
                .filter_map(move |at| {
                    let (class_tag, _) =
                        lp_ascii_filtered(bytes, *at, 0..=2000, u8::is_ascii_graphic)?;
                    Some(DesignRecordHeader {
                        id: String::new(),
                        record_index,
                        class_tag,
                        byte_offset: *at as u64,
                    })
                })
        })
        .collect()
}

pub(crate) fn parameter_scope_tail_length_is_valid(kind: &str, tail_length: usize) -> bool {
    if (80..=590).contains(&tail_length) && tail_length.is_multiple_of(2) {
        return true;
    }
    match kind {
        "CopyPasteBodies" => tail_length == 110,
        "CoilPrimitive" => matches!(tail_length, 72 | 76 | 77 | 78 | 87 | 88),
        _ => matches!(tail_length, 72 | 76 | 77 | 78 | 87),
    }
}

pub(crate) fn parameter_scope_previous_history_offset(
    kind: &str,
    tail_length: usize,
) -> Option<usize> {
    parameter_scope_previous_history_offset_for_form(kind, tail_length, false)
}

fn parameter_scope_previous_history_offset_for_form(
    kind: &str,
    tail_length: usize,
    named_tail: bool,
) -> Option<usize> {
    if named_tail {
        return None;
    }
    match (kind, tail_length) {
        ("CopyPasteBodies", 110) => Some(53),
        ("CoilPrimitive", 88) => None,
        (_, 72 | 76) => Some(30),
        (_, 77 | 78) => Some(31),
        (_, 87) => Some(41),
        _ => None,
    }
}

pub(crate) fn parse_parameter_scope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    header: &DesignRecordHeader,
) -> Option<DesignParameterScope> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let paired_at = records.first_at_or_after(start.checked_add(11)?, header.record_index)?;
    let (paired_class_tag, _) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let mut candidates = Vec::new();
    let mut tail_candidates = vec![
        (72, false),
        (76, false),
        (77, false),
        (78, false),
        (87, false),
        (88, false),
        (110, false),
    ];
    tail_candidates.extend((0..=256).map(|label_code_units| (78 + label_code_units * 2, true)));
    for (tail_length, named_tail) in tail_candidates {
        let Some(end) = paired_at.checked_sub(tail_length) else {
            continue;
        };
        let earliest = end.saturating_sub(4 + 2 * 256).max(start + 11);
        for at in earliest..end {
            let Some((kind, decoded_end)) = lp_utf16_bounded(bytes, at, 1..=256) else {
                continue;
            };
            let named_tail_valid = !named_tail
                || named_parameter_scope_tail_is_valid(bytes, decoded_end, paired_at, tail_length)
                    .is_some_and(|valid| valid);
            if decoded_end == end
                && (parameter_scope_tail_length_is_valid(&kind, tail_length)
                    || named_tail && tail_length == 78)
                && kind.chars().all(|character| !character.is_control())
                && named_tail_valid
            {
                candidates.push((at, end, tail_length, kind, named_tail));
            }
        }
    }
    if candidates.iter().filter(|candidate| candidate.4).count() == 1 {
        candidates.retain(|candidate| candidate.4);
    }
    let [(kind_at, kind_end, tail_length, kind, named_tail)] = candidates.as_slice() else {
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
        match parameter_scope_previous_history_offset_for_form(kind, *tail_length, *named_tail) {
            Some(offset) => Some(kind_end.checked_add(offset)?),
            None => None,
        };
    let previous_history_state_id =
        previous_history_state_id_offset.and_then(|offset| match u32_at(bytes, offset)? {
            u32::MAX => None,
            state_id => Some(i64::from(state_id)),
        });
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
        exact_surface_stitch_operation(bytes, records, header.record_index, reference_members)
    } else {
        None
    };
    let surface_patch_boundaries = if kind == "SurfacePatch" {
        super::patch::surface_patch_boundaries(bytes, records, reference_members)
    } else {
        Vec::new()
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
    let ruled_surface_operation = if kind == "SurfaceRuled" {
        exact_ruled_surface_operation(
            bytes,
            start,
            paired_at,
            *reference_count_at,
            reference_members,
        )
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
        exact_extrude_prologue(bytes, start, *reference_count_at, reference_members)
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
        exact_coil_discriminators(bytes, start, paired_at, kind, reference_members).map_or(
            (None, None, None, None, None, None, None, None, None, None),
            |fields| {
                (
                    Some(fields.operation),
                    Some(fields.operation_offset),
                    fields.extent,
                    fields.extent_offset,
                    Some(fields.section),
                    fields.section_offset,
                    Some(fields.section_placement),
                    fields.section_placement_offset,
                    Some(fields.clockwise),
                    fields.clockwise_offset,
                )
            },
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
        previous_history_state_id_offset: previous_history_state_id_offset
            .and_then(|offset| u64::try_from(offset).ok())
            .unwrap_or_default(),
        reference_count_offset: u64::try_from(*reference_count_at).ok()?,
        reference_members: reference_members.clone(),
        reference_member_offsets: reference_member_offsets.clone(),
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation,
        surface_patch_boundaries,
        base_flange_operation,
        edge_flange_operation,
        hem_operation,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_position: None,
        work_point_position_offset: None,
        unclosed_construction_operand_groups: Vec::new(),
        work_point_reference_type: None,
        work_point_input_record_indices: Vec::new(),
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

fn named_parameter_scope_tail_is_valid(
    bytes: &[u8],
    kind_end: usize,
    paired_at: usize,
    tail_length: usize,
) -> Option<bool> {
    let label_at = kind_end.checked_add(8)?;
    let (label, label_end) = lp_utf16_bounded(bytes, label_at, 0..=256)?;
    let label_code_units = label.encode_utf16().count();
    if tail_length != 78usize.checked_add(label_code_units.checked_mul(2)?)?
        || label_end.checked_add(7)? != kind_end.checked_add(19 + label_code_units * 2)?
        || label.chars().any(char::is_control)
    {
        return Some(false);
    }
    let marker = kind_end.checked_add(19 + label_code_units.checked_mul(2)?)?;
    if marker.checked_add(59)? != paired_at || bytes.get(label_end..marker)? != [0; 7] {
        return Some(false);
    }
    Some(
        bytes.get(kind_end + 4..kind_end + 8)? == [0; 4]
            && bytes.get(marker) == Some(&1)
            && bytes.get(marker + 1).is_some_and(|field_id| *field_id != 0)
            && read_u64(bytes, marker + 2)? == 1
            && bytes.get(marker + 10..marker + 12)? == [0; 2]
            && u32_at(bytes, marker + 12)? > 0
            && u32_at(bytes, marker + 16)? == 0xfc
            && f64_at(bytes, marker + 20)?.is_finite()
            && u32_at(bytes, marker + 28)? == 0xfc
            && bytes.get(marker + 32) == Some(&1)
            && bytes
                .get(marker + 33)
                .is_some_and(|field_id| *field_id != 0)
            && read_u64(bytes, marker + 34)? == 1
            && bytes.get(marker + 42..marker + 46)? == [0, 1, 0, 0]
            && bytes.get(marker + 46) == Some(&1)
            && bytes
                .get(marker + 47)
                .is_some_and(|field_id| *field_id != 0)
            && read_u64(bytes, marker + 48)? == 1
            && bytes.get(marker + 56..marker + 59)? == [0; 3],
    )
}

/// Decode the fixed discriminator block of the closed Coil scope forms.
///
/// A scope whose envelope is valid but whose Coil dialect is not recognized
/// still belongs in the native arena. Returning `None` here leaves its
/// family-local fields unset without discarding the ordered references and
/// byte span that preserve the unsupported form.
struct CoilDiscriminators {
    operation: DesignExtrudeOperation,
    operation_offset: u64,
    extent: Option<DesignCoilExtent>,
    extent_offset: Option<u64>,
    section: DesignCoilSection,
    section_offset: Option<u64>,
    section_placement: DesignCoilSectionPlacement,
    section_placement_offset: Option<u64>,
    clockwise: bool,
    clockwise_offset: Option<u64>,
}

fn exact_coil_discriminators(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &str,
    reference_members: &[u32],
) -> Option<CoilDiscriminators> {
    if let Some(fields) =
        exact_long_coil_discriminators(bytes, start, paired_at, kind, reference_members)
    {
        return Some(fields);
    }
    let operation_offset = start.checked_add(20)?;
    let operation = match (kind, u32_at(bytes, operation_offset)?) {
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
    let structural_constant = match kind {
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
    let section_placement_offset = start.checked_add(107)?;
    let (section, section_placement) = match kind {
        "SpirePrimitive" => (
            match u32_at(bytes, section_offset)? {
                0 => DesignCoilSection::Circular,
                1 => DesignCoilSection::Square,
                2 => DesignCoilSection::ExternalTriangle,
                3 => DesignCoilSection::InternalTriangle,
                _ => return None,
            },
            match u32_at(bytes, section_placement_offset)? {
                4 => DesignCoilSectionPlacement::Inside,
                _ => return None,
            },
        ),
        // The compact Coil dialect stores the two discriminators in the
        // opposite lanes from SpirePrimitive: position at offset 92 and
        // section shape at offset 107.
        "CoilPrimitive" => (
            match u32_at(bytes, section_placement_offset)? {
                1 => DesignCoilSection::Circular,
                2 => DesignCoilSection::Square,
                3 => DesignCoilSection::ExternalTriangle,
                4 => DesignCoilSection::InternalTriangle,
                _ => return None,
            },
            match u32_at(bytes, section_offset)? {
                1 => DesignCoilSectionPlacement::Inside,
                2 => DesignCoilSectionPlacement::Center,
                3 => DesignCoilSectionPlacement::Outside,
                _ => return None,
            },
        ),
        _ => return None,
    };
    Some(CoilDiscriminators {
        operation,
        operation_offset: operation_offset as u64,
        extent: Some(extent),
        extent_offset: Some(extent_offset as u64),
        section,
        section_offset: Some(section_offset as u64),
        section_placement,
        section_placement_offset: Some(section_placement_offset as u64),
        clockwise,
        clockwise_offset: Some(clockwise_offset as u64),
    })
}

fn exact_long_coil_discriminators(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &str,
    reference_members: &[u32],
) -> Option<CoilDiscriminators> {
    if kind != "CoilPrimitive" || reference_members.len() != 10 {
        return None;
    }
    let frame_length = paired_at.checked_sub(start)?;
    if !matches!(frame_length, 450 | 578)
        || bytes.get(start.checked_add(11)?..start.checked_add(22)?)? != [0; 11]
        || u32_at(bytes, start.checked_add(26)?)? != 1
        || marked_record_reference(bytes, start.checked_add(30)?)? != *reference_members.get(4)?
        || marked_record_reference(bytes, start.checked_add(41)?)? != *reference_members.get(8)?
    {
        return None;
    }
    let operation_value = u32_at(bytes, start.checked_add(22)?)?;
    let operation = match (frame_length, operation_value) {
        (450, 1) => DesignExtrudeOperation::Join,
        (450, 2) => DesignExtrudeOperation::Cut,
        (450, 3) => DesignExtrudeOperation::Intersect,
        (578, 2) if exact_long_coil_matrix(bytes, start) => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    Some(CoilDiscriminators {
        operation,
        operation_offset: u64::try_from(start.checked_add(22)?).ok()?,
        // The long form has no extent selector. Its exact owned parameter set
        // supplies the mode after the scope is parsed.
        extent: None,
        extent_offset: None,
        // The long form fixes these settings in its dialect envelope.
        section: DesignCoilSection::Circular,
        section_offset: None,
        section_placement: DesignCoilSectionPlacement::Inside,
        section_placement_offset: None,
        clockwise: false,
        clockwise_offset: None,
    })
}

fn exact_long_coil_matrix(bytes: &[u8], start: usize) -> bool {
    let Some(values) = f64s_at(bytes, start.saturating_add(77), 16) else {
        return false;
    };
    values.iter().all(|value| value.is_finite())
        && values[12..15].iter().all(|value| *value == 0.0)
        && values[15] == 1.0
}

fn bind_coil_extent_from_parameters(
    scope: &mut DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::DesignParameterOwner],
) {
    if scope.kind != "CoilPrimitive" || scope.coil_extent.is_some() {
        return;
    }
    let Some(stream) = native_stream(&scope.id) else {
        return;
    };
    let mut owned_kinds = parameter_owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
        })
        .filter_map(|owner| {
            parameters
                .iter()
                .find(|parameter| {
                    native_stream(&parameter.id) == Some(stream)
                        && parameter.record_index == owner.parameter_record_index
                })
                .map(|parameter| (owner.local_ordinal, parameter.source_kind.as_str()))
        })
        .collect::<Vec<_>>();
    owned_kinds.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    let owned_kinds = owned_kinds
        .into_iter()
        .map(|(_, source_kind)| source_kind)
        .collect::<Vec<_>>();
    let extent = match owned_kinds.as_slice() {
        ["Diameter", "SectionSize", "TaperAngle", "Revolutions", "Height"]
        | ["Diameter", "SectionSize", "TaperAngle", "Height", "Revolutions"] => {
            Some(DesignCoilExtent::RevolutionsHeight)
        }
        ["Diameter", "SectionSize", "TaperAngle", "Revolutions", "Pitch"]
        | ["Diameter", "SectionSize", "TaperAngle", "Pitch", "Revolutions"] => {
            Some(DesignCoilExtent::RevolutionsPitch)
        }
        ["Diameter", "SectionSize", "TaperAngle", "Height", "Pitch"]
        | ["Diameter", "SectionSize", "TaperAngle", "Pitch", "Height"] => {
            Some(DesignCoilExtent::HeightPitch)
        }
        ["Diameter", "SectionSize", "Revolutions", "Pitch"]
        | ["Diameter", "SectionSize", "Pitch", "Revolutions"] => Some(DesignCoilExtent::Spiral),
        _ => None,
    };
    scope.coil_extent = extent;
}

fn exact_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignExtrudePrologue> {
    exact_current_extrude_prologue(bytes, start)
        .or_else(|| {
            exact_legacy_shifted_extrude_prologue(
                bytes,
                start,
                reference_count_at,
                reference_members,
            )
        })
        .or_else(|| exact_legacy_distance_extrude_prologue(bytes, start, reference_count_at))
}

fn exact_legacy_distance_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
) -> Option<DesignExtrudePrologue> {
    let marker_offset = start.checked_add(20)?;
    let (prefix_value, prefix_value_offset, operation_offset, expected_reference_count_delta) =
        match bytes.get(marker_offset)? {
            0 => (None, None, marker_offset.checked_add(1)?, 208),
            1 => {
                let prefix_value_offset = marker_offset.checked_add(1)?;
                let prefix_value = u32_at(bytes, prefix_value_offset)?;
                if prefix_value != 0 {
                    return None;
                }
                (
                    Some(prefix_value),
                    Some(prefix_value_offset),
                    prefix_value_offset.checked_add(4)?,
                    212,
                )
            }
            _ => return None,
        };
    if reference_count_at.checked_sub(start)? != expected_reference_count_delta {
        return None;
    }
    let operation = match u32_at(bytes, operation_offset)? {
        1 => DesignExtrudeOperation::Join,
        2 => DesignExtrudeOperation::Cut,
        3 => DesignExtrudeOperation::Intersect,
        4 => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let extent_discriminator_offset = operation_offset.checked_add(4)?;
    let extent_discriminator = u32_at(bytes, extent_discriminator_offset)?;
    if extent_discriminator != 2 {
        return None;
    }
    let direction_reversed_offset = extent_discriminator_offset.checked_add(4)?;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let geometry_kind_offset = direction_reversed_offset.checked_add(1)?;
    let geometry_kind = u32_at(bytes, geometry_kind_offset)?;
    if !matches!(geometry_kind, 0 | 1) {
        return None;
    }
    Some(DesignExtrudePrologue::LegacyDistance {
        prefix_value,
        prefix_value_offset: prefix_value_offset.map(|offset| offset as u64),
        operation,
        operation_offset: operation_offset as u64,
        extent_discriminator,
        extent_discriminator_offset: extent_discriminator_offset as u64,
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        geometry_kind,
        geometry_kind_offset: geometry_kind_offset as u64,
    })
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
                    && matches!(bytes.get(operation_offset.saturating_add(13)), Some(0 | 1))
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
    let solid_operation_offset = operation_offset.checked_add(13)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
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
        solid_operation,
        solid_operation_offset: solid_operation_offset as u64,
        start,
        start_offset: start_offset as u64,
    })
}

fn exact_legacy_shifted_extrude_prologue(
    bytes: &[u8],
    start: usize,
    reference_count_at: usize,
    reference_members: &[u32],
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
    let direction_face_extend_values = [
        u32_at(bytes, first_extent_offset)?,
        u32_at(bytes, second_extent_offset)?,
    ];
    if !matches!(direction_face_extend_values[0], 1..=3) {
        return None;
    }
    let two_sided_offsets = || {
        let first_parameter_at = start.checked_add(139)?;
        let first_side_extent_offset = start.checked_add(155)?;
        let first_offset_at = start.checked_add(159)?;
        let second_side_extent_offset = start.checked_add(178)?;
        let second_parameter_at = start.checked_add(182)?;
        if second_parameter_at.checked_add(11)? > reference_count_at
            || bytes.get(start.checked_add(150)?..first_side_extent_offset)? != [0; 5]
            || bytes.get(start.checked_add(170)?..second_side_extent_offset)? != [0; 8]
            || [first_parameter_at, first_offset_at, second_parameter_at]
                .into_iter()
                .map(|offset| marked_record_reference(bytes, offset))
                .any(|reference| !reference.is_some_and(|value| reference_members.contains(&value)))
        {
            return None;
        }
        Some([first_side_extent_offset, second_side_extent_offset])
    };
    let extent_for =
        |discriminators: [u32; 2]| match (direction_face_extend_values[0], discriminators) {
            (1, [1, 0]) => Some(DesignExtrudeExtent::OneSidedDistance),
            (1, [2, 0]) => Some(DesignExtrudeExtent::OneSidedToFace),
            (1, [3, 0]) => Some(DesignExtrudeExtent::OneSidedThroughNext),
            (1, [4, 0]) => Some(DesignExtrudeExtent::OneSidedThroughAll),
            (2, [1, 1]) => Some(DesignExtrudeExtent::TwoSidedDistance),
            (3, [1, 0]) => Some(DesignExtrudeExtent::SymmetricDistance),
            (3, [4, 4]) => Some(DesignExtrudeExtent::SymmetricThroughAll),
            _ => None,
        };
    let candidate = |first_side_extent_offset: usize, default_second_offset: usize| {
        if first_side_extent_offset.checked_add(4)? > reference_count_at {
            return None;
        }
        let first_side_extent = u32_at(bytes, first_side_extent_offset)?;
        let second_side_extent_offset = if first_side_extent == 2 {
            reference_count_at.checked_sub(4)?
        } else {
            default_second_offset
        };
        if second_side_extent_offset.checked_add(4)? > reference_count_at {
            return None;
        }
        let offsets = [first_side_extent_offset, second_side_extent_offset];
        let discriminators = [u32_at(bytes, offsets[0])?, u32_at(bytes, offsets[1])?];
        extent_for(discriminators).map(|extent| (offsets, discriminators, extent))
    };
    let (side_extent_discriminator_offsets, side_extent_discriminators, extent) =
        if direction_face_extend_values[0] == 2 {
            let offsets = two_sided_offsets()?;
            let discriminators = [u32_at(bytes, offsets[0])?, u32_at(bytes, offsets[1])?];
            (offsets, discriminators, extent_for(discriminators)?)
        } else {
            let (first_offset, second_offset) = match reference_count_at.checked_sub(start)? {
                252 | 262 | 263 => (106, 110),
                272 => (116, 130),
                294 => (116, 129),
                _ => return None,
            };
            candidate(
                start.checked_add(first_offset)?,
                start.checked_add(second_offset)?,
            )?
        };
    let direction_reversed_offset = operation_offset.checked_add(12)?;
    let direction_reversed = match bytes.get(direction_reversed_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let solid_operation_offset = operation_offset.checked_add(13)?;
    let solid_operation = match bytes.get(solid_operation_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
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
        direction_face_extend_values,
        side_extent_discriminators,
        side_extent_discriminator_offsets: [
            side_extent_discriminator_offsets[0] as u64,
            side_extent_discriminator_offsets[1] as u64,
        ],
        extent: Some(extent),
        direction_face_extend_offsets: [first_extent_offset as u64, second_extent_offset as u64],
        direction_reversed,
        direction_reversed_offset: direction_reversed_offset as u64,
        solid_operation,
        solid_operation_offset: solid_operation_offset as u64,
        start,
        start_offset: start_offset as u64,
    })
}

pub(crate) fn exact_surface_stitch_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    references: &[u32],
) -> Option<DesignSurfaceStitchOperation> {
    if references.len() < 4 || !references.len().is_multiple_of(2) {
        return None;
    }
    let tolerance_record_index = references[references.len() - 2];
    let settings_record_index = references[references.len() - 1];
    let scalar = exact_fixed_scalar(bytes, records, tolerance_record_index)?;
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

pub(crate) fn exact_ruled_surface_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    reference_count_at: usize,
    reference_members: &[u32],
) -> Option<DesignRuledSurfaceOperation> {
    if bytes.get(start.checked_add(11)?..start.checked_add(20)?)? != [0; 9] {
        return None;
    }
    let method_offset = start.checked_add(20)?;
    let method = match u32_at(bytes, method_offset)? {
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
        (cursor == at.checked_add(11)?
            && reference.segment.is_none()
            && reference.link_name.is_none())
        .then(|| u32::try_from(reference.target?).ok())?
    };
    let angle_owner_record_index = fixed_reference(start.checked_add(28)?)?;
    let distance_owner_record_index = fixed_reference(start.checked_add(39)?)?;
    let corner_offset = start.checked_add(50)?;
    let corner = match u32_at(bytes, corner_offset)? {
        0 => DesignRuledSurfaceCorner::Rounded,
        1 => DesignRuledSurfaceCorner::Mitered,
        _ => return None,
    };
    let take_reference_list = |mut cursor: usize| {
        let count = usize::try_from(u32_at(bytes, cursor)?).ok()?;
        if count > 100_000 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(fixed_reference(cursor)?);
            cursor = cursor.checked_add(11)?;
        }
        Some((records, cursor))
    };
    let (mut edge_group_record_indices, mut cursor) = take_reference_list(start.checked_add(54)?)?;
    if u32_at(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (auxiliary_record_indices, next) = take_reference_list(cursor)?;
    cursor = next;
    if u32_at(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (trailing_edge_groups, next) = take_reference_list(cursor)?;
    cursor = next;
    edge_group_record_indices.extend(trailing_edge_groups);
    let (direction_entity_id, direction_end) = lp_utf16_bounded(bytes, cursor, 36..=36)?;
    let direction_absent = direction_entity_id == "00000000-0000-0000-0000-000000000000";
    if direction_end.checked_add(3)? != reference_count_at
        || bytes.get(direction_end..reference_count_at)? != [0; 3]
        || paired_at <= reference_count_at
        || (!direction_absent && !crate::bytes::is_guid_relaxed(&direction_entity_id))
    {
        return None;
    }
    let direction_entity_id = (!direction_absent).then_some(direction_entity_id);
    if reference_members.first() != Some(&distance_owner_record_index)
        || reference_members.get(1) != Some(&angle_owner_record_index)
        || edge_group_record_indices.is_empty()
        || edge_group_record_indices
            .iter()
            .any(|record_index| !reference_members.contains(record_index))
    {
        return None;
    }
    Some(DesignRuledSurfaceOperation {
        method,
        method_offset: method_offset as u64,
        corner,
        corner_offset: corner_offset as u64,
        alternate_face,
        alternate_face_offset: alternate_face_offset as u64,
        angle_owner_record_index,
        distance_owner_record_index,
        edge_group_record_indices,
        auxiliary_record_indices,
        direction_entity_id,
    })
}

/// Optional four-byte scope-header member widths that shift the fixed operation
/// section of a sheet-metal edge treatment.
///
/// The member is not announced by another field, so the true offset of the fixed
/// section is settled by reference agreement instead: exactly one candidate
/// makes every marked slot name a record the ordered reference table lists.
const SHEET_METAL_HEADER_SHIFTS: [usize; 2] = [0, 4];

/// Largest width-distance parameter-owner count a sheet-metal edge-width mode adds.
///
/// The full-edge mode adds none, the symmetric mode one, and the two-sided mode
/// two. A higher count belongs to a frame form this reader does not account for.
const MAX_EDGE_WIDTH_DISTANCE_OWNERS: usize = 2;

pub(crate) fn exact_edge_flange_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignEdgeFlangeOperation> {
    // The header shift is recovered by agreement, so both candidates are
    // evaluated and a frame that reads under either one is refused as ambiguous.
    let mut resolved = None;
    for header_shift in SHEET_METAL_HEADER_SHIFTS {
        for candidate in [
            edge_flange_operation_at(bytes, start, paired_at, references, header_shift),
            edge_flange_to_object_operation_at(bytes, start, paired_at, references, header_shift),
        ]
        .into_iter()
        .flatten()
        {
            if resolved.is_some() {
                return None;
            }
            resolved = Some(candidate);
        }
    }
    resolved
}

/// Read the `EdgeFlange` fixed operation section for one candidate header shift
/// and refuse the candidate unless every slot agrees.
fn edge_flange_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignEdgeFlangeOperation> {
    // The ordered reference table is in record-index order, so no role has a
    // fixed table position. Every role is instead named by a marked slot in the
    // fixed operation section, and the operand of a group is the record three
    // after it. The table entries no role claims are the width-distance
    // parameter owners the edge-width mode adds.
    //
    // Only the single-edge form is accounted for. A frame selecting more edges
    // names one edge group and one aggregate group in the same two slots, so
    // neither the further groups nor the order of their operands against the
    // aggregate operands is established, and such a frame is refused.
    if references.len() < 8 {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    let bend_position = DesignBendPosition::from_code(u32_at(bytes, common)?);
    if u32_at(bytes, common.checked_add(4)?)? != 1 {
        return None;
    }
    // Every reference the fixed section names is removed from this pool, so the
    // entries that remain at the end are exactly the unclaimed ones.
    let mut unclaimed: Vec<u32> = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };

    let mut cursor = common.checked_add(8)?;
    let edge_wrapper_record_indices = vec![claim(
        marked_record_reference(bytes, cursor)?,
        &mut unclaimed,
    )?];
    cursor = cursor.checked_add(11)?;
    let settings_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = cursor.checked_add(11)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(u32_at(bytes, cursor)?);
    cursor = cursor.checked_add(4)?;
    let angle_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = cursor.checked_add(11)?;
    let height_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = cursor.checked_add(11)?;
    let reference_side_code = u32_at(bytes, cursor)?;
    let bend_radius_offset = cursor.checked_add(15)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let result_count = usize::try_from(u32_at(bytes, bend_radius_offset.checked_add(14)?)?).ok()?;
    // The aggregate-group and role-`0x08` group slots close the section after the
    // result-record run, so they also confirm the recovered result count.
    let aggregate_slot = bend_radius_offset
        .checked_add(22)?
        .checked_add(result_count.checked_mul(15)?)?;
    let aggregate_group_record_index = claim(
        marked_record_reference(bytes, aggregate_slot)?,
        &mut unclaimed,
    )?;
    let first_edge_group = marked_record_reference(bytes, aggregate_slot.checked_add(27)?)?;

    // A group's recipe-backed operand is the record three after the group.
    let aggregate_operand_record_indices = vec![claim(
        aggregate_group_record_index.checked_add(3)?,
        &mut unclaimed,
    )?];
    let edge_group_record_indices = vec![claim(first_edge_group, &mut unclaimed)?];
    let edge_operand_record_indices =
        vec![claim(first_edge_group.checked_add(3)?, &mut unclaimed)?];

    if unclaimed.len() > MAX_EDGE_WIDTH_DISTANCE_OWNERS {
        return None;
    }
    let width_count = unclaimed.len();
    let width_distance_owner_record_indices = unclaimed;

    let expected_length = 493usize
        .checked_add(result_count.checked_mul(15)?)?
        .checked_add(width_count.checked_mul(11)?)?
        .checked_add(header_shift)?;
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
        height_extent: DesignEdgeFlangeHeightExtent::Distance,
        angle_owner_record_index,
        width_distance_owner_record_indices,
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        reference_side_code,
        height_datum,
        bend_position,
    })
}

/// Read the single-edge `EdgeFlange` form whose height is measured from a
/// selected construction entity.
fn edge_flange_to_object_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignEdgeFlangeOperation> {
    // This form has one target group and one target entity-selection operand in
    // addition to the distance form's roles. The two marked references between
    // the target group and the aggregate group are fixed-frame references, not
    // entries in the scope's ordered reference table, and are retained as
    // native references for rewrite.
    if references.len() != 11 {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    let bend_position = DesignBendPosition::from_code(u32_at(bytes, common)?);
    if u32_at(bytes, common.checked_add(4)?)? != 1 {
        return None;
    }
    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let mut cursor = common.checked_add(8)?;
    let edge_wrapper_record_indices = vec![claim(
        marked_record_reference(bytes, cursor)?,
        &mut unclaimed,
    )?];
    cursor = cursor.checked_add(11)?;
    let settings_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = cursor.checked_add(11)?;
    let height_datum = DesignSheetMetalHeightDatum::from_code(u32_at(bytes, cursor)?);
    cursor = cursor.checked_add(4)?;
    let angle_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = cursor.checked_add(11)?;
    let height_owner_record_index = claim(marked_record_reference(bytes, cursor)?, &mut unclaimed)?;
    cursor = cursor.checked_add(11)?;
    let reference_side_code = u32_at(bytes, cursor)?;
    let bend_radius_offset = cursor.checked_add(15)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let result_count = u32_at(bytes, bend_radius_offset.checked_add(14)?)?;
    if result_count != 1
        || bytes.get(bend_radius_offset.checked_add(18)?..bend_radius_offset.checked_add(22)?)?
            != [0; 4]
    {
        return None;
    }
    if bytes.get(common.checked_add(89)?..common.checked_add(94)?)? != [0; 5] {
        return None;
    }
    let target_group_record_index = claim(
        marked_record_reference(bytes, common.checked_add(94)?)?,
        &mut unclaimed,
    )?;
    if u32_at(bytes, common.checked_add(105)?)? != 2 {
        return None;
    }
    let reference_record_indices = [
        marked_record_reference(bytes, common.checked_add(109)?)?,
        marked_record_reference(bytes, common.checked_add(124)?)?,
    ];
    if reference_record_indices[0] == reference_record_indices[1]
        || reference_record_indices
            .iter()
            .any(|record_index| references.contains(record_index))
        || u32_at(bytes, common.checked_add(120)?)? != 1
        || bytes.get(common.checked_add(135)?..common.checked_add(139)?)? != [0; 4]
        || u32_at(bytes, common.checked_add(139)?)? != 1
        || bytes.get(common.checked_add(154)?..common.checked_add(166)?)? != [0; 12]
        || u32_at(bytes, common.checked_add(166)?)? != 1
    {
        return None;
    }
    let aggregate_group_record_index = claim(
        marked_record_reference(bytes, common.checked_add(143)?)?,
        &mut unclaimed,
    )?;
    let edge_group_record_index = claim(
        marked_record_reference(bytes, common.checked_add(170)?)?,
        &mut unclaimed,
    )?;
    let target_operand_record_index =
        claim(target_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let aggregate_operand_record_indices = vec![claim(
        aggregate_group_record_index.checked_add(3)?,
        &mut unclaimed,
    )?];
    let edge_group_record_indices = vec![edge_group_record_index];
    let edge_operand_record_indices = vec![claim(
        edge_group_record_index.checked_add(3)?,
        &mut unclaimed,
    )?];
    let [offset_owner_record_index] = unclaimed.as_slice() else {
        return None;
    };
    let expected_length = 576usize.checked_add(header_shift)?;
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
        height_extent: DesignEdgeFlangeHeightExtent::ToObject {
            target_group_record_index,
            target_operand_record_index,
            offset_owner_record_index: *offset_owner_record_index,
            reference_record_indices,
        },
        angle_owner_record_index,
        width_distance_owner_record_indices: Vec::new(),
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        reference_side_code,
        height_datum,
        bend_position,
    })
}

pub(crate) fn exact_hem_operation(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
) -> Option<DesignHemOperation> {
    // The header shift is recovered by agreement, so both candidates are
    // evaluated and a frame that reads under either one is refused as ambiguous.
    let mut resolved = None;
    for header_shift in SHEET_METAL_HEADER_SHIFTS {
        for candidate in [
            hem_gap_length_operation_at(bytes, start, paired_at, references, header_shift),
            hem_radius_angle_operation_at(bytes, start, paired_at, references, header_shift),
            hem_gap_length_radius_operation_at(bytes, start, paired_at, references, header_shift),
        ]
        .into_iter()
        .flatten()
        {
            if resolved.is_some() {
                return None;
            }
            resolved = Some(candidate);
        }
    }
    resolved
}

/// Read the gap-and-length `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// The ordered reference table is in record-index order, so every role is taken
/// from the marked slot that names it and each group's operand is the record
/// three after that group. The rolled and teardrop forms place their owner
/// references at other offsets and are handled by their corresponding readers.
fn hem_gap_length_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignHemOperation> {
    if references.len() != 8
        || paired_at.checked_sub(start)? != 494usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if u32_at(bytes, common.checked_add(4)?)? != 1 {
        return None;
    }

    let mut unclaimed: Vec<u32> = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Option<u32> {
        claim(
            marked_record_reference(bytes, common.checked_add(offset)?)?,
            pool,
        )
    };

    let edge_wrapper_record_index = slot(8, &mut unclaimed)?;
    let settings_record_index = slot(19, &mut unclaimed)?;
    // The two owners are the form's inputs in local-ordinal order.
    let gap_owner_record_index = slot(42, &mut unclaimed)?;
    let length_owner_record_index = slot(53, &mut unclaimed)?;

    let bend_radius_offset = common.checked_add(71)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }

    let aggregate_group_record_index = slot(108, &mut unclaimed)?;
    let edge_group_record_index = slot(135, &mut unclaimed)?;
    let aggregate_operand_record_index =
        claim(aggregate_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let edge_operand_record_index = claim(edge_group_record_index.checked_add(3)?, &mut unclaimed)?;
    if !unclaimed.is_empty() {
        return None;
    }

    Some(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index,
        edge_operand_record_index,
        aggregate_group_record_index,
        aggregate_operand_record_index,
        parameter_owners: DesignHemParameterOwners::GapLength {
            gap_owner_record_index,
            length_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        form_code: u32_at(bytes, common)?,
        direction_code: u32_at(bytes, common.checked_add(30)?)?,
        direction_reversal_byte: *bytes.get(common.checked_add(34)?)?,
        reference_side_code: u32_at(bytes, common.checked_add(36)?)?,
    })
}

/// Read the rolled `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
///
/// Rolled forms keep the two-owner frame length, but their owner slots are at
/// offsets `41` and `54` instead of `42` and `53`. The source parameter kinds
/// assign those slots to radius and angle; the fixed frame only proves their
/// record identities.
fn hem_radius_angle_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignHemOperation> {
    if references.len() != 8
        || paired_at.checked_sub(start)? != 494usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if u32_at(bytes, common.checked_add(4)?)? != 1 {
        return None;
    }

    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Option<u32> {
        claim(
            marked_record_reference(bytes, common.checked_add(offset)?)?,
            pool,
        )
    };

    let edge_wrapper_record_index = slot(8, &mut unclaimed)?;
    let settings_record_index = slot(19, &mut unclaimed)?;
    let angle_owner_record_index = slot(41, &mut unclaimed)?;
    let radius_owner_record_index = slot(54, &mut unclaimed)?;
    let bend_radius_offset = common.checked_add(71)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let aggregate_group_record_index = slot(108, &mut unclaimed)?;
    let edge_group_record_index = slot(135, &mut unclaimed)?;
    let aggregate_operand_record_index =
        claim(aggregate_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let edge_operand_record_index = claim(edge_group_record_index.checked_add(3)?, &mut unclaimed)?;
    if !unclaimed.is_empty() {
        return None;
    }

    Some(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index,
        edge_operand_record_index,
        aggregate_group_record_index,
        aggregate_operand_record_index,
        parameter_owners: DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index,
            angle_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        form_code: u32_at(bytes, common)?,
        direction_code: u32_at(bytes, common.checked_add(30)?)?,
        direction_reversal_byte: *bytes.get(common.checked_add(34)?)?,
        reference_side_code: u32_at(bytes, common.checked_add(36)?)?,
    })
}

/// Read the teardrop `Hem` fixed operation section for one candidate header
/// shift and refuse the candidate unless every slot agrees.
fn hem_gap_length_radius_operation_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    references: &[u32],
    header_shift: usize,
) -> Option<DesignHemOperation> {
    if references.len() != 9
        || paired_at.checked_sub(start)? != 515usize.checked_add(header_shift)?
    {
        return None;
    }
    let common = start.checked_add(85)?.checked_add(header_shift)?;
    if u32_at(bytes, common.checked_add(4)?)? != 1 {
        return None;
    }

    let mut unclaimed = references.to_vec();
    let claim = |index: u32, pool: &mut Vec<u32>| -> Option<u32> {
        let at = pool.iter().position(|entry| *entry == index)?;
        pool.remove(at);
        Some(index)
    };
    let slot = |offset: usize, pool: &mut Vec<u32>| -> Option<u32> {
        claim(
            marked_record_reference(bytes, common.checked_add(offset)?)?,
            pool,
        )
    };

    let edge_wrapper_record_index = slot(8, &mut unclaimed)?;
    let settings_record_index = slot(19, &mut unclaimed)?;
    let gap_owner_record_index = slot(42, &mut unclaimed)?;
    let length_owner_record_index = slot(53, &mut unclaimed)?;
    let radius_owner_record_index = slot(64, &mut unclaimed)?;
    let bend_radius_offset = common.checked_add(81)?;
    let bend_radius = f64_at(bytes, bend_radius_offset)?;
    if !bend_radius.is_finite() || bend_radius <= 0.0 {
        return None;
    }
    let aggregate_group_record_index = slot(118, &mut unclaimed)?;
    let edge_group_record_index = slot(145, &mut unclaimed)?;
    let aggregate_operand_record_index =
        claim(aggregate_group_record_index.checked_add(3)?, &mut unclaimed)?;
    let edge_operand_record_index = claim(edge_group_record_index.checked_add(3)?, &mut unclaimed)?;
    if !unclaimed.is_empty() {
        return None;
    }

    Some(DesignHemOperation {
        edge_wrapper_record_index,
        edge_group_record_index,
        edge_operand_record_index,
        aggregate_group_record_index,
        aggregate_operand_record_index,
        parameter_owners: DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index,
            length_owner_record_index,
            radius_owner_record_index,
        },
        settings_record_index,
        bend_radius,
        bend_radius_offset: u64::try_from(bend_radius_offset).ok()?,
        form_code: u32_at(bytes, common)?,
        direction_code: u32_at(bytes, common.checked_add(30)?)?,
        direction_reversal_byte: *bytes.get(common.checked_add(34)?)?,
        reference_side_code: u32_at(bytes, common.checked_add(36)?)?,
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

#[cfg(test)]
mod tests {
    use super::{
        exact_pattern_identity_wrapper, exact_work_point_position, parse_parameter_scope,
        POINT_DATA_TYPE_GUID,
    };
    use crate::design::decode::sketch::IndexedRecordOffsets;
    use crate::records::{DesignParameterScope, DesignRecordHeader};
    use std::collections::HashMap;

    fn lp_utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    #[test]
    fn circular_pattern_identity_wrapper_closes_on_its_persistent_identity() {
        fn header(bytes: &mut Vec<u8>, class_tag: &str, record_index: u32) {
            bytes.extend_from_slice(&3u32.to_le_bytes());
            bytes.extend_from_slice(class_tag.as_bytes());
            bytes.extend_from_slice(&record_index.to_le_bytes());
        }
        fn marked(bytes: &mut Vec<u8>, record_index: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record_index.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }

        let mut bytes = Vec::new();
        let record_index = 80;
        header(&mut bytes, "308", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&40u64.to_le_bytes());
        lp_utf16(&mut bytes, "384d79a0-c23e-42aa-b993-74df1f8dfcae");
        lp_utf16(&mut bytes, "352c47d7-42ba-443e-9de1-ae0e37cc129d");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        marked(&mut bytes, record_index + 1);
        header(&mut bytes, "305", record_index + 1);
        bytes.extend_from_slice(&[0; 10]);
        marked(&mut bytes, record_index + 2);
        header(&mut bytes, "300", record_index + 2);
        bytes.extend_from_slice(&[0; 10]);
        let identity_offset = bytes.len();
        bytes.extend_from_slice(&503u64.to_le_bytes());
        header(&mut bytes, "308", record_index + 3);

        assert_eq!(
            exact_pattern_identity_wrapper(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                record_index,
            ),
            Some((503, identity_offset as u64))
        );
        bytes[identity_offset - 1] = 1;
        assert_eq!(
            exact_pattern_identity_wrapper(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                record_index,
            ),
            None
        );
    }

    /// A `WorkPoint` scope record, its paired header, and one point-data record
    /// frame: the indexed header, the payload prologue with an optional property
    /// block, the class-level members of `version`, a base-level run of `inputs`
    /// references, and the second header that closes the frame.
    fn work_point_stream(
        class_tag: &str,
        version: u32,
        property: bool,
        pick_point: Option<u64>,
        position: [f64; 3],
        reference_type: u32,
        inputs: u32,
    ) -> (Vec<u8>, DesignParameterScope, usize) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"427");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&55u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "WorkPoint");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 11]);

        bytes.extend_from_slice(&(class_tag.len() as u32).to_le_bytes());
        bytes.extend_from_slice(class_tag.as_bytes());
        bytes.extend_from_slice(&55u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(0);
        if property {
            bytes.push(1);
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&6u32.to_le_bytes());
            bytes.extend_from_slice(b"pt_tag");
            bytes.extend_from_slice(&23u32.to_le_bytes());
            bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
            bytes.extend_from_slice(&9u64.to_le_bytes());
        } else {
            bytes.push(0);
        }
        if version >= 2 {
            bytes.extend_from_slice(&0i32.to_le_bytes());
        }
        for _ in 0..2 {
            bytes.extend_from_slice(&f64::to_le_bytes(0.0));
        }
        if version >= 1 {
            match pick_point {
                Some(target) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&target.to_le_bytes());
                    bytes.extend_from_slice(&[0, 0]);
                }
                None => bytes.push(0),
            }
        }
        let position_at = bytes.len();
        for value in position {
            bytes.extend_from_slice(&f64::to_le_bytes(value));
        }
        bytes.extend_from_slice(&reference_type.to_le_bytes());
        if version >= 3 {
            for _ in 0..3 {
                bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
            }
        }
        bytes.extend_from_slice(&inputs.to_le_bytes());
        for input in 0..inputs {
            bytes.push(1);
            bytes.extend_from_slice(&u64::from(70 + input).to_le_bytes());
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&55u32.to_le_bytes());

        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "427".into(),
            byte_offset: 0,
        };
        let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
            .expect("WorkPoint scope");
        (bytes, scope, position_at)
    }

    #[test]
    fn work_point_position_survives_a_property_block_and_a_present_pick_point() {
        let (bytes, scope, position_at) =
            work_point_stream("282", 3, true, Some(9), [1.25, -2.5, 3.75], 20, 1);

        let frame = exact_work_point_position(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &HashMap::new(),
        )
        .expect("work point frame");
        assert_eq!(frame.position, [1.25, -2.5, 3.75]);
        assert_eq!(frame.position_offset, position_at as u64);
        assert_eq!(frame.input_record_indices, [70]);
    }

    #[test]
    fn work_point_position_reads_every_class_version_that_stores_one() {
        for version in 0..=3 {
            let (bytes, scope, position_at) =
                work_point_stream("282", version, false, None, [4.0, 5.0, 6.0], 5, 1);

            let frame = exact_work_point_position(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                &scope,
                &HashMap::new(),
            )
            .unwrap_or_else(|| panic!("class version {version}"));
            assert_eq!(frame.position, [4.0, 5.0, 6.0], "class version {version}");
            assert_eq!(
                frame.position_offset, position_at as u64,
                "class version {version}"
            );
        }
    }

    #[test]
    fn work_point_reads_the_class_version_its_type_table_stores() {
        let (bytes, scope, position_at) =
            work_point_stream("282", 2, false, None, [4.0, 5.0, 6.0], 5, 1);
        let records = IndexedRecordOffsets::build(&bytes);
        let frame = exact_work_point_position(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 2))]),
        )
        .expect("work point frame");
        assert_eq!(frame.position, [4.0, 5.0, 6.0]);
        assert_eq!(frame.position_offset, position_at as u64);
        // The stored version drives the read: a version that describes a
        // different member sequence does not yield this frame's coordinate.
        assert_ne!(
            exact_work_point_position(
                &bytes,
                &records,
                &scope,
                &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 0))])
            )
            .map(|frame| frame.position_offset),
            Some(position_at as u64)
        );
        // An unregistered entity falls back to the agreement sweep.
        assert_eq!(
            exact_work_point_position(
                &bytes,
                &records,
                &scope,
                &HashMap::from([(9, (POINT_DATA_TYPE_GUID, 0))])
            ),
            exact_work_point_position(&bytes, &records, &scope, &HashMap::new())
        );
    }

    #[test]
    fn work_point_position_does_not_depend_on_the_segment_local_class_tag() {
        // A class tag is `256` plus an index into the segment's own type table,
        // so the point-data class wears a different tag in every segment. The
        // coordinate is the same wherever the type table names the class.
        for class_tag in ["282", "316", "364", "409", "424", "460", "468"] {
            let (bytes, scope, position_at) =
                work_point_stream(class_tag, 2, false, None, [7.5, 8.5, 9.5], 5, 1);
            let records = IndexedRecordOffsets::build(&bytes);

            let frame = exact_work_point_position(
                &bytes,
                &records,
                &scope,
                &HashMap::from([(55, (POINT_DATA_TYPE_GUID, 2))]),
            )
            .unwrap_or_else(|| panic!("class tag {class_tag}"));
            assert_eq!(frame.position, [7.5, 8.5, 9.5], "class tag {class_tag}");
            assert_eq!(
                frame.position_offset, position_at as u64,
                "class tag {class_tag}"
            );
        }
    }

    #[test]
    fn work_point_rejects_a_registered_entity_of_another_type() {
        // The tag says `282`, but the type table names a different class for
        // this entity, so the record is not point data whatever its tag reads.
        let (bytes, scope, _) = work_point_stream("282", 2, false, None, [4.0, 5.0, 6.0], 5, 1);
        let records = IndexedRecordOffsets::build(&bytes);

        assert_eq!(
            exact_work_point_position(
                &bytes,
                &records,
                &scope,
                &HashMap::from([(55, ("A0A15D26-1F3B-4120-A3F1-9CDDA189AB74", 2))])
            ),
            None
        );
    }

    #[test]
    fn work_point_uses_the_serialized_input_count_for_every_rule() {
        // The input count is a member of the point-data level. It frames the
        // run independently of the rule selector, including three-input
        // constructions.
        let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 18, 1);

        let records = IndexedRecordOffsets::build(&bytes);
        let frame = exact_work_point_position(&bytes, &records, &scope, &HashMap::new())
            .expect("work point frame");
        assert_eq!(frame.input_record_indices, [70]);
        assert_eq!(frame.reference_type, 18);
        let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 14, 2);
        let frame = exact_work_point_position(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &HashMap::new(),
        )
        .expect("work point frame");
        assert_eq!(frame.input_record_indices, [70, 71]);

        let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 8, 3);
        let frame = exact_work_point_position(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &HashMap::new(),
        )
        .expect("work point frame");
        assert_eq!(frame.input_record_indices, [70, 71, 72]);

        let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 18, 2);
        let frame = exact_work_point_position(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &HashMap::new(),
        )
        .expect("work point frame");
        assert_eq!(frame.input_record_indices, [70, 71]);
    }
}
