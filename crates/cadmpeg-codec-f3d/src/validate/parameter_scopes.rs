// SPDX-License-Identifier: Apache-2.0
//! Validation of parameter scopes and their feature operands.

use super::assembly::{
    as_built_locator_layout, as_built_operand_frame_link, operand_list_combine_offsets_hold,
    rigid_alignment_scalars_hold, rigid_group_members_link, valid_assembly_operand_path_link,
    valid_axial_assembly_targets, valid_class_307_joint_origin_qualifier,
    valid_class_363_operand_path_link, valid_component_insert_role,
    valid_component_pattern_occurrences,
};
use super::links::{thread_class_pair_holds, thread_designation_offset_holds};
use super::work_geometry::{valid_work_plane_construction, valid_work_point_construction};
use super::{
    design_stream, push_constant_finding, Ctx, EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10,
    EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8,
};
use crate::design::decode::scopes::extrude::{
    is_class_296_legacy_one_sided_distance_layout, is_class_296_legacy_one_sided_to_face_layout,
    is_class_296_one_sided_to_face_layout, is_class_296_symmetric_distance_layout,
    is_class_296_two_sided_to_faces_layout, through_all,
};
use crate::design::decode::scopes::legacy_class_397::Class397SymmetricFrame;
use crate::design::decode::scopes::legacy_class_415;
use crate::layout::{
    assembly_operand_path_wrapper as path_wrapper,
    class_296_261_legacy_extrude_prefix as class_296_legacy_prefix,
    class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance,
    class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face,
    class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face,
    class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric,
    class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces,
    legacy_class_397_symmetric_extrude_frame as class_397,
    legacy_class_415_symmetric_extrude_prefix as class_415,
};
use crate::records::feature::scope::{DesignJointOriginTransform, DesignParameterScope};
use crate::records::feature::work_geometry::joint_origin::DirectJointOriginCarrier;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::{design, ids, records};
use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use std::collections::{HashMap, HashSet};

/// Validate feature parameter scopes and their paired feature-operation frames.
/// The links resolve into design-intent records, so a document without them
/// has nothing to check.
pub(super) fn validate_parameter_scopes(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    if !ctx.holds_design_intent() {
        return Ok(());
    }
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let mut scope_indices = HashSet::new();
    let mut stream_types_by_stream = std::collections::HashMap::new();
    for scope in &native.design_parameter_scopes {
        let native_stream = design_stream(&scope.id);
        let stream_types = stream_types_by_stream
            .entry(native_stream)
            .or_insert_with(|| {
                native_stream
                    .strip_prefix("f3d:")
                    .and_then(ids::decode_identity_key_component)
                    .map(|entry| {
                        design::decode::meta::stream_types_by_entity(&native.design_types, &entry)
                    })
                    .unwrap_or_default()
            });
        let unique_index = scope_indices.insert((native_stream, scope.record_index));
        let entity_link = scope.sketch_entity().map(|binding| {
            entities_by_suffix
                .get(&(native_stream, binding.entity_id.suffix()))
                .is_some_and(|entity| {
                    entity.entity_id == binding.entity_id
                        && binding.entity_reference_offset > scope.byte_offset()
                        && binding.entity_reference_offset < scope.paired_byte_offset()
                })
        });
        let valid_profile_frame =
            |profile: &records::topology::sketch_profile::DesignSketchProfileOperand,
             allow_absent: bool| {
                let header = records_by_index.get(&(native_stream, profile.record_index));
                let entity = entities_by_suffix.get(&(native_stream, profile.entity_id.suffix()));
                usize::try_from(profile.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&profile.record_index)
                    && header.is_some_and(|header| {
                        header.byte_offset == profile.byte_offset()
                            && header.class_tag == profile.class_tag
                    })
                    && if profile.sketch_absent {
                        allow_absent && entity.is_none()
                    } else {
                        entity.is_some_and(|entity| {
                            entity.in_sketch_module() && entity.entity_id == profile.entity_id
                        })
                    }
                    && profile.region_selection.as_ref().is_none_or(|selection| {
                        super::sketch_scope::profile_region_link(scope, profile, selection)
                    })
            };
        let valid_sketch_profile =
            |profile: &records::topology::sketch_profile::DesignSketchProfileOperand| {
                valid_profile_frame(profile, false)
            };
        let native_profiles_link = scope.extrude_native_profiles().is_none_or(|profiles| {
            (profiles.len() > 1 || profiles.iter().any(|profile| profile.sketch_absent))
                && scope.extrude_profile().is_none()
                && profiles
                    .iter()
                    .all(|profile| valid_profile_frame(profile, true))
                && profiles
                    .iter()
                    .map(|profile| profile.record_index)
                    .collect::<HashSet<_>>()
                    .len()
                    == profiles.len()
        });
        let extrude_profile_link =
            scope.extrude_profile().is_none_or(valid_sketch_profile) && native_profiles_link;

        let sweep_profile_link = scope.sweep_profile().is_none_or(valid_sketch_profile);
        let base_flange_profile_link = scope.base_flange_profile().is_none_or(valid_sketch_profile);
        let emboss_profile_link = scope
            .emboss_operand_construction()
            .and_then(|construction| construction.sketch_profile.as_ref())
            .is_none_or(valid_sketch_profile);
        let move_link = design::decode::scopes::direct_face::move_operation_links_hold(
            scope,
            scope.move_operation().and_then(|operation| {
                records_by_index
                    .get(&(native_stream, operation.transform_record_index))
                    .copied()
            }),
        );
        let split_face_link = scope.split_face_operation().is_none_or(|operation| {
            operation.scope_offsets_valid(scope.byte_offset(), scope.reference_count_offset())
                && operation.carrier_frame.is_none_or(|frame| {
                    operation.direction_reference.local_record_index().is_some_and(|record_index| {
                        records_by_index.get(&(native_stream, record_index)).is_some_and(|header| header.byte_offset == frame.primary_offset)
                            && stream_types.get(&u64::from(record_index)).is_some_and(|(guid, version)| {
                                guid.eq_ignore_ascii_case(records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID) && *version == frame.version
                            })
                            && native_stream.strip_suffix("BulkStream.dat").is_some_and(|prefix| {
                                native.design_types.iter().filter(|row| {
                                    ids::native_stream(row.id()).and_then(|stream| stream.strip_suffix("MetaStream.dat")) == Some(prefix)
                                }).flat_map(|row| row.entities.values()).filter(|id| **id == u64::from(record_index)).count() == 1
                            })
                            && native.design_record_headers.iter().filter(|header| design_stream(&header.id) == native_stream && header.byte_offset > frame.primary_offset).map(|header| header.byte_offset).min().is_none_or(|next| frame.end_offset <= next)
                    })
                })
        });
        let offset_faces_link = match scope.payload() {
            records::feature::scope::DesignScopePayload::OffsetFaces(Some(operation)) => {
                design::decode::scopes::direct_face::offset_faces_distance_reference_matches(
                    scope,
                    operation.distance_record_index,
                    |record_index| {
                        native
                            .design_construction_operand_groups
                            .iter()
                            .any(|group| {
                                design_stream(&group.id) == native_stream
                                    && group.scope_record_index == scope.record_index
                                    && group.record_index == record_index
                                    && group.role() == DesignOperandRole::ROLE_0X10
                            })
                    },
                )
            }
            _ => true,
        };
        let fold_links = scope
            .contour_flange_operation()
            .is_none_or(|operation| operation.valid_for_scope(scope))
            && scope
                .unfold_operation()
                .is_none_or(|operation| operation.valid_for_scope(scope))
            && scope
                .refold_operation()
                .is_none_or(|operation| operation.valid_for_scope(scope));
        let base_flange_link = match scope.base_flange_operation() {
            None => true,
            Some(operation) => {
                scope.reference_members().values().copied().eq([
                    operation.profile_group_record_index,
                    operation.profile_record_index,
                    operation.thickness_record_index,
                    operation.settings_record_index,
                ]) && scope.base_flange_profile().is_some_and(|profile| {
                    profile.record_index == operation.profile_record_index
                        && profile.scope_reference_ordinal == 1
                }) && u64::try_from(crate::layout::base_flange_scope::THICKNESS_COPY)
                    .ok()
                    .and_then(|offset| scope.byte_offset().checked_add(offset))
                    .is_some_and(|expected_offset| operation.thickness_offset == expected_offset)
                    && operation.thickness_offset < scope.paired_byte_offset()
            }
        };
        let edge_flange_link = match scope.edge_flange_operation() {
            None => scope
                .edge_flange_construction()
                .is_none_or(|construction| construction.valid_for_scope(scope)),
            Some(operation) => {
                // The ordered reference table is in record-index order, so the
                // check is that every role names a distinct table entry and that
                // the entries no role claims are exactly the width owners.
                let edge_count = operation.selection.shape().edges().count();
                let claimed = operation
                    .selection
                    .shape()
                    .edges()
                    .flat_map(|edge| {
                        [
                            edge.wrapper,
                            edge.group_record_index.get(),
                            edge.operand_record_index(),
                            edge.aggregate_operand_record_index,
                        ]
                    })
                    .chain(operation.selection.shape().owner_indices().copied())
                    .chain(operation.auxiliary_reference_record_indices.iter().copied())
                    .chain([
                        operation.selection.aggregate_group_record_index(),
                        operation.height_owner_record_index,
                        operation.angle_owner_record_index,
                        operation.settings_record_index,
                    ])
                    .collect::<Vec<_>>();
                let mut claimed = claimed;
                if let records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } = operation.selection.shape().height()
                {
                    for index in [
                        target_group_record_index,
                        target_operand_record_index,
                        offset_owner_record_index,
                    ] {
                        claimed.push(index);
                    }
                }
                let unique_claimed = claimed
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>();
                let corner_layout =
                    matches!(scope.class_tag.as_str(), "361" | "362") && edge_count > 1;
                let corner_form = !corner_layout
                    || (scope.paired_class_tag.as_str() == "258"
                        && design::decode::scopes::sheet_metal::corner_flange::operation_holds(
                            operation,
                            scope.byte_offset(),
                            scope.frame_length(),
                        )
                        && scope
                            .reference_members()
                            .values()
                            .zip(scope.reference_members().values().skip(1))
                            .all(|(left, right)| left < right));
                corner_form
                    && edge_count > 0
                    && claimed.len() == scope.reference_members().len()
                    && unique_claimed.len() == claimed.len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let hem_link = match scope.hem_operation() {
            None => true,
            Some(operation) => {
                let mut claimed = vec![
                    operation.edge_wrapper_record_index,
                    operation.edge_group_record_index.get(),
                    operation.edge_operand_record_index(),
                    operation.aggregate_group_record_index.get(),
                    operation.aggregate_operand_record_index(),
                    operation.settings_record_index,
                ];
                match &operation.parameter_owners {
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLength {
                        gap_owner_record_index,
                        length_owner_record_index,
                    } => claimed.extend([*gap_owner_record_index, *length_owner_record_index]),
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::RadiusAngle {
                        radius_owner_record_index,
                        angle_owner_record_index,
                    } => claimed.extend([*radius_owner_record_index, *angle_owner_record_index]),
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLengthRadius {
                        gap_owner_record_index,
                        length_owner_record_index,
                        radius_owner_record_index,
                    } => claimed.extend([
                        *gap_owner_record_index,
                        *length_owner_record_index,
                        *radius_owner_record_index,
                    ]),
                }
                claimed.iter().copied().collect::<HashSet<_>>().len() == claimed.len()
                    && claimed.len() == scope.reference_members().len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let copy_paste_link = match scope.copy_paste_bodies_operation() {
            None => true,
            Some(operation) => {
                let group_header =
                    records_by_index.get(&(native_stream, operation.body_group_record_index));
                let relation_header =
                    records_by_index.get(&(native_stream, operation.relation_record_index));
                scope.reference_members().values().next()
                    == Some(&operation.body_group_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .skip(1)
                        .copied()
                        .eq(operation.bodies().iter().map(|body| body.operand.value))
                    && group_header.is_some_and(|header| {
                        header.byte_offset == operation.body_group_byte_offset()
                            && header.class_tag == operation.body_group_class_tag
                    })
                    && relation_header.is_some_and(|header| {
                        header.byte_offset == operation.relation_byte_offset()
                            && header.class_tag == operation.relation_class_tag
                    })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.source.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                            }) || native.design_body_members.iter().any(|member| {
                                design_stream(member.id()) == native_stream
                                    && member.entity_suffix == u64::from(suffix)
                            })
                        })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.copied.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                                    && (binding.body.is_some() || binding.history_only)
                            })
                        })
            }
        };
        let rectangular_pattern_link = match scope.rectangular_pattern_construction() {
            None => true,
            Some(construction) => {
                let instances_link = construction.instances().is_none_or(|instances| {
                    let active = [
                        (construction.u_count(), construction.u_extent()),
                        (construction.v_count(), construction.v_extent()),
                    ]
                    .into_iter()
                    .filter(|(count, _)| *count > 1)
                    .collect::<Vec<_>>();
                    let [(count, extent)] = active.as_slice() else {
                        return false;
                    };
                    let Ok(count) = usize::try_from(*count) else {
                        return false;
                    };
                    let Some(reference_end) = count.checked_add(5) else {
                        return false;
                    };
                    let expected_records = scope
                        .reference_members()
                        .values()
                        .next()
                        .into_iter()
                        .chain(
                            scope
                                .reference_members()
                                .values_in(6..reference_end)
                                .into_iter()
                                .flatten(),
                        )
                        .copied();
                    let Some(first) = instances
                        .frames()
                        .next()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let Some(last) = instances
                        .frames()
                        .next_back()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let delta = [
                        last[0][3] - first[0][3],
                        last[1][3] - first[1][3],
                        last[2][3] - first[2][3],
                    ];
                    let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
                    let component_link =
                        valid_component_pattern_occurrences(native, native_stream, instances);
                    instances
                        .frames()
                        .map(|frame| frame.record_index)
                        .eq(expected_records)
                        && instances.instance_count() == count
                        && instances.frames().all(|frame| {
                            let transform = &frame.transform.value;
                            (0..3).all(|row| {
                                (0..3).all(|column| {
                                    (transform[row][column] - first[row][column]).abs()
                                        <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10
                                })
                            })
                        })
                        && (distance - extent.abs()).abs()
                            <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                        && instances.frames().enumerate().all(|(ordinal, frame)| {
                            let transform = &frame.transform.value;
                            let (Some(ordinal), Some(divisor)) =
                                (f64_from_index(ordinal), f64_from_index(count - 1))
                            else {
                                return false;
                            };
                            let fraction = ordinal / divisor;
                            (0..3).all(|axis| {
                                (transform[axis][3] - first[axis][3] - delta[axis] * fraction).abs()
                                    <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                            })
                        })
                        && instances.frames().all(|frame| {
                            records_by_index
                                .get(&(native_stream, frame.record_index))
                                .is_some_and(|header| frame.transform.offset > header.byte_offset)
                        })
                        && component_link
                });
                instances_link
                    && native
                        .design_parameter_owners
                        .iter()
                        .filter(|owner| {
                            design_stream(owner.id()) == native_stream
                                && owner.scope_record_index() == scope.record_index
                        })
                        .count()
                        == 4
                    && construction
                        .owner_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                                && records_by_index.contains_key(&(native_stream, *record_index))
                        })
                    && construction
                        .owner_record_indices
                        .iter()
                        .zip(construction.value_offsets)
                        .zip([
                            f64::from(construction.u_count()),
                            f64::from(construction.v_count()),
                            construction.u_extent(),
                            construction.v_extent(),
                        ])
                        .enumerate()
                        .all(|(ordinal, ((record_index, value_offset), value))| {
                            native.design_parameter_owners.iter().any(|owner| {
                                design_stream(owner.id()) == native_stream
                                    && owner.record_index() == *record_index
                                    && owner.scope_record_index() == scope.record_index
                                    && u32::try_from(ordinal) == Ok(owner.local_ordinal())
                                    && owner.evaluated_value().get() == value
                                    && owner.evaluated_value_offset() == value_offset
                            })
                        })
            }
        };
        let direct_joint_origin_carrier_link =
            |target: &DesignParameterScope, origin: &DesignJointOriginTransform| {
                let Some(evidence) = &origin.carrier else {
                    return false;
                };
                DirectJointOriginCarrier::for_scope(target)
                    .into_iter()
                    .flatten()
                    .any(|carrier| {
                        evidence.frame_length == u64_from_index(carrier.length)
                            && carrier.matches_offsets(origin)
                            && carrier.has_registered_types(target, stream_types)
                            && carrier
                                .construction_index(
                                    &evidence.prefix,
                                    &evidence.scope_backlink,
                                    target.record_index,
                                )
                                .is_some_and(|index| {
                                    origin.reference.as_ref().is_some_and(|reference| {
                                        reference.joint_origin_reference == index
                                    }) && records_by_index.contains_key(&(native_stream, index))
                                })
                            && records_by_index
                                .get(&(native_stream, carrier.record_index))
                                .is_some_and(|header| header.byte_offset == carrier.start)
                    })
            };
        let assembly_alignment_link = match scope.assembly_alignment() {
            None => true,
            Some(alignment)
                if design::decode::scopes::assembly_v3::registered(scope, stream_types)
                    && alignment.frame_sense.is_some() =>
            {
                design::decode::scopes::assembly_v3::valid_alignment(
                    scope,
                    alignment,
                    stream_types,
                    &native.design_parameter_owners,
                )
            }
            Some(alignment) => {
                let values = if alignment.owners.len() == 2 {
                    vec![alignment.angle(), alignment.offset()[2]]
                } else if design::assembly::placement_ordered_as_built_scope(scope) {
                    vec![
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                        alignment.angle(),
                    ]
                } else {
                    vec![
                        alignment.angle(),
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                    ]
                };
                let generation = design::assembly::AssemblyScopeGeneration::for_scope(scope);
                let operand_frame_variant = generation.operand_frame_variant();
                let compact_as_built = design::assembly::compact_placement_as_built_scope(scope);
                let variable_reference = !compact_as_built
                    && !design::assembly::rigid_as_built_scope(scope)
                    && design::assembly::variable_reference_assembly_generation(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                    );
                let compact_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Compact)
                );
                let axial_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Axial)
                );
                let wide_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Wide)
                );
                let compact_wide_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::CompactWide)
                );
                let wide_as_built = design::assembly::wide_as_built_scope(scope);
                let rigid_as_built = design::assembly::rigid_as_built_scope(scope);
                let as_built_frames = scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                    && (scope.frame_length() == 399 || wide_as_built || compact_as_built);
                let as_built_421_generation = design::assembly::legacy_as_built_421_generation(
                    scope.frame_length(),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                );
                let as_built_421 = as_built_421_generation.is_some();
                let operand_paths = alignment.operand_path_refs();
                let wide_family_offsets = operand_frame_variant
                    .and_then(crate::design::decode::scopes::wide_family_frame_offsets);
                let frame_reference_offsets =
                    if let Some([first, _, second, _]) = wide_family_offsets {
                        [u64_from_index(first + 1), u64_from_index(second + 1)]
                    } else if axial_frames {
                        [29, 168]
                    } else if compact_frames {
                        [25, 165]
                    } else {
                        [29, 169]
                    };
                let frame_transform_offsets =
                    if let Some([_, first, _, second]) = wide_family_offsets {
                        [u64_from_index(first), u64_from_index(second)]
                    } else if axial_frames {
                        [39, 178]
                    } else if compact_frames {
                        [36, 176]
                    } else {
                        [40, 180]
                    };
                let assembly_owner_count = native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .count();
                let alignment_lane_bounds = generation.alignment_lane_bounds(assembly_owner_count);
                let operand_frames_link = if let Some(
                    records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                        carriers,
                        ..
                    },
                ) = &alignment.form
                {
                    carriers
                        .references()
                        .into_iter()
                        .enumerate()
                        .all(|(ordinal, reference)| {
                            let reference_ordinal = ordinal * 2;
                            scope
                                .reference_members()
                                .values()
                                .nth(reference_ordinal)
                                .copied()
                                == Some(reference.value)
                                && scope
                                    .reference_members()
                                    .offsets()
                                    .nth(reference_ordinal)
                                    .copied()
                                    == Some(reference.offset)
                                && records_by_index.contains_key(&(native_stream, reference.value))
                        })
                } else {
                    alignment.operand_frames().is_none_or(|frames| {
                        frames[0].reference_record_index != frames[1].reference_record_index
                            && frames.iter().enumerate().all(|(ordinal, frame)| {
                                let offsets_match = if as_built_frames {
                                    operand_paths.as_ref().is_some_and(|paths| {
                                        as_built_operand_frame_link(
                                            scope,
                                            paths[ordinal],
                                            frame,
                                            stream_types,
                                        )
                                    })
                                } else {
                                    Some(frame.reference_offset)
                                        == scope
                                            .byte_offset()
                                            .checked_add(frame_reference_offsets[ordinal])
                                        && Some(frame.transform_offset)
                                            == scope
                                                .byte_offset()
                                                .checked_add(frame_transform_offsets[ordinal])
                                };
                                let reference_exists = if as_built_frames {
                                    frame.reference_record_index != 0
                                } else {
                                    records_by_index.contains_key(&(
                                        native_stream,
                                        frame.reference_record_index,
                                    ))
                                };
                                offsets_match && reference_exists
                            })
                    })
                };
                let solved_frame_link = alignment.solved_frame().is_none_or(|frame| {
                    let Some(generation) = as_built_421_generation else {
                        return false;
                    };
                    let Some(header) =
                        records_by_index.get(&(native_stream, frame.reference_record_index))
                    else {
                        return false;
                    };
                    as_built_421
                        && scope.reference_members().values().nth(8).copied()
                            == Some(frame.reference_record_index)
                        && scope.reference_members().offsets().nth(8).copied()
                            == Some(frame.reference_offset)
                        && header.class_tag.as_str() == generation.frame_class_tag()
                        && frame.class_tag == header.class_tag
                        && frame.record_byte_offset == header.byte_offset
                        && Some(frame.transform_offset)
                            == frame
                                .record_byte_offset
                                .checked_add(u64_from_index(generation.matrix_offset()))
                });
                let operand_qualifiers_link = match alignment.form.as_ref() {
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(
                        operands,
                    )) => {
                        let frames = operands.each_ref().map(|operand| operand.frame.clone());
                        match (&operands[0].qualifier, &operands[1].qualifier) {
                            (records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: second }) => {
                                let paths = [first, second];

                            let class_363_carriers = paths
                                .iter()
                                .all(|path| path.link().locator_class_tag.as_str() == "363");
                            if class_363_carriers {
                                !axial_frames
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && paths.iter().zip(&frames).all(|(path, frame)| {
                                        valid_class_363_operand_path_link(scope, frame, path)
                                    })
                            } else {
                                let locator_offsets =
                                    generation.operand_path_locator_offsets();
                                let first_start = paths[0].link().locator_byte_offset;
                                let second_start = paths[1].link().locator_byte_offset;
                                let counted_wrapper = variable_reference || wide_as_built || compact_as_built
                                    || operand_frame_variant.is_some_and(
                                        design::assembly::AssemblyOperandFrameVariant::is_wide_family,
                                    );
                                let envelope_ends = paths.each_ref().map(|path| {
                                    let link = path.link();
                                    let skipped_index = (rigid_as_built || compact_as_built)
                                        && design::assembly::typed_as_built_path_wrapper_gap(
                                            link.locator_record_index,
                                            path.record_index,
                                            link.wrapper_record_index,
                                            stream_types,
                                        )
                                        && path.two_keys_in_one_typed_record();
                                    let continuation_count = if counted_wrapper && !skipped_index {
                                        link.wrapper_record_index
                                            .checked_sub(link.locator_record_index)?
                                            .checked_sub(2)?
                                    } else {
                                        0
                                    };
                                    let stride = if rigid_as_built || compact_as_built {
                                        as_built_locator_layout(scope, path, stream_types)?.reference_stride()
                                    } else { 11 };
                                    u64::try_from(path_wrapper::PATH_REFERENCE + stride)
                                        .ok()?
                                        .checked_add(u64::from(continuation_count).checked_mul(u64_from_index(stride))?)
                                        .and_then(|length| {
                                            path.link().wrapper_byte_offset.checked_add(length)
                                        })
                                });
                                !axial_frames
                                    && locator_offsets.is_some_and(|offsets| {
                                        paths.iter().zip(offsets).all(|(path, offset)| {
                                            valid_assembly_operand_path_link(scope, path, offset, stream_types)
                                        })
                                    })
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && matches!(envelope_ends, [Some(first_end), Some(second_end)]
                                if !(first_start < second_end && second_start < first_end))
                                    && paths.iter().all(|path| {
                                        (path.admitted_by_type()
                                            || matches!(path.class_tag().as_str(), "294" | "299" | "307" | "329" | "330" | "386" | "390"))
                                            && !matches!(path.link().locator_class_tag.as_str(), "363" | "378")
                                            && (path.admitted_by_type()
                                                || matches!(
                                                path.class_tag().as_str(),
                                                "294" | "299" | "307" | "330" | "386"
                                            ) || path.occurrence_guids().first().is_some_and(
                                                |guid| {
                                                    native
                                                        .design_component_occurrences
                                                        .iter()
                                                        .filter(|occurrence| {
                                                            design_stream(&occurrence.id)
                                                                == native_stream
                                                                && occurrence
                                                                    .occurrence_guid.as_str()
                                                                    .eq_ignore_ascii_case(guid.value.as_str())
                                                        })
                                                        .count()
                                                        == 1
                                                },
                                            ))
                                    })
                            }

                            }
                            (records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: second }) => {
                                axial_frames && valid_axial_assembly_targets(native, records_by_index, native_stream, scope, &frames, &[first, second])
                            }
                            _ if variable_reference && operands.iter().any(|operand| matches!(operand.qualifier, records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. })) => {
                                frames[0].reference_record_index != frames[1].reference_record_index
                                    && operands.iter().all(|operand| match &operand.qualifier {
                                        records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } => valid_class_363_operand_path_link(scope, &operand.frame, path),
                                        qualifier @ records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. } => valid_class_307_joint_origin_qualifier(native, records_by_index, native_stream, &operand.frame, qualifier),
                                        records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. } => false,
                                    })
                            }
                            _ => false,
                        }
                    }
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Frames {
                        ..
                    }) => axial_frames,
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::LimitsOnly {
                        ..
                    }) => as_built_421,
                    None
                    | Some(
                        records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                            ..
                        },
                    ) => true,
                };
                let joint_origin_envelope_link = alignment
                    .joint_origin_scope_record_index()
                    .is_none_or(|record_index| {
                        scope.class_tag.as_str() == "276"
                            && scope.paired_class_tag.as_str() == "258"
                            && scope.frame_length() == 604
                            && native.design_parameter_scopes.iter().any(|target| {
                                design_stream(&target.id) == native_stream
                                    && target.kind()
                                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                    && target.record_index == record_index
                                    && target.joint_origin_frame().is_some_and(|origin| {
                                        if origin.carrier.is_some() {
                                            direct_joint_origin_carrier_link(target, origin)
                                        } else {
                                            scope.byte_offset().checked_add(36)
                                                == Some(origin.joint_origin_transform_offset)
                                        }
                                    })
                            })
                    });
                let alignment_scalars_link = if let Some(generation) = as_built_421_generation {
                    match (alignment.limits(), alignment.owners.as_slice()) {
                        (Some(limits), [angle, offset_x, offset_y, offset_z]) => {
                            let alignment_lanes = [
                                (
                                    offset_x.value,
                                    offset_x.offset,
                                    alignment.offset()[0],
                                    0_u32,
                                ),
                                (
                                    offset_y.value,
                                    offset_y.offset,
                                    alignment.offset()[1],
                                    1_u32,
                                ),
                                (
                                    offset_z.value,
                                    offset_z.offset,
                                    alignment.offset()[2],
                                    2_u32,
                                ),
                                (angle.value, angle.offset, alignment.angle(), 3_u32),
                            ];
                            let limit_order = if generation.reverse_limit_order() {
                                [1, 0]
                            } else {
                                [0, 1]
                            };
                            let limit_lanes = limit_order.into_iter().zip([4_u32, 5]).map(
                                |(index, local_ordinal)| {
                                    (
                                        limits.owner_record_indices[index],
                                        limits.value_offsets[index],
                                        [limits.minimum(), limits.maximum()][index],
                                        local_ordinal,
                                    )
                                },
                            );
                            limits.kind == generation.limit_kind()
                                && alignment_lanes.into_iter().chain(limit_lanes).all(
                                    |(record_index, value_offset, value, local_ordinal)| {
                                        native.design_parameter_owners.iter().any(|owner| {
                                            design_stream(owner.id()) == native_stream
                                                && owner.record_index() == record_index
                                                && owner.scope_record_index() == scope.record_index
                                                && owner.local_ordinal() == local_ordinal
                                                && owner.evaluated_value().get() == value
                                                && owner.evaluated_value_offset() == value_offset
                                        })
                                    },
                                )
                        }
                        _ => false,
                    }
                } else {
                    rigid_alignment_scalars_hold(alignment, rigid_as_built)
                        && alignment_lane_bounds.is_some_and(|(alignment_start, alignment_end)| {
                            alignment_end.checked_sub(alignment_start).is_some_and(
                                |expected_offset| alignment.owners.len() == expected_offset,
                            ) && alignment.owners.iter().zip(&values).enumerate().all(
                                |(ordinal, (lane, value))| {
                                    native.design_parameter_owners.iter().any(|owner| {
                                        design_stream(owner.id()) == native_stream
                                            && owner.record_index() == lane.value
                                            && owner.scope_record_index() == scope.record_index
                                            && u32::try_from(alignment_start + ordinal)
                                                == Ok(owner.local_ordinal())
                                            && owner.evaluated_value().get() == *value
                                            && owner.evaluated_value_offset() == lane.offset
                                    })
                                },
                            )
                        })
                };
                let alignment_reference_link = if let Some(generation) = as_built_421_generation {
                    alignment.limits().is_some_and(|limits| {
                        let limit_reference_indices = if generation.reverse_limit_order() {
                            [
                                limits.owner_record_indices[1],
                                limits.owner_record_indices[0],
                            ]
                        } else {
                            limits.owner_record_indices
                        };
                        alignment.owners.len() == 4
                            && scope
                                .reference_members()
                                .values()
                                .skip(4)
                                .take(4)
                                .copied()
                                .eq([
                                    alignment.owners[1].value,
                                    alignment.owners[2].value,
                                    alignment.owners[3].value,
                                    alignment.owners[0].value,
                                ])
                            && scope
                                .reference_members()
                                .values()
                                .skip(9)
                                .take(2)
                                .eq(limit_reference_indices.iter())
                    })
                } else if wide_frames || compact_wide_frames {
                    // The wide scope lists its four owners first.
                    scope
                        .reference_members()
                        .values()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value))
                } else if variable_reference {
                    (0..scope.reference_members().len())
                        .filter(|&start| {
                            scope
                                .reference_members()
                                .values_in(start..start + alignment.owners.len())
                                .is_some_and(|values| {
                                    values.eq(alignment.owners.iter().map(|owner| &owner.value))
                                })
                        })
                        .count()
                        == 1
                } else if design::assembly::placement_ordered_as_built_scope(scope) {
                    let mut owners = native
                        .design_parameter_owners
                        .iter()
                        .filter(|owner| {
                            design_stream(owner.id()) == native_stream
                                && owner.scope_record_index() == scope.record_index
                        })
                        .map(|owner| (owner.local_ordinal(), owner.record_index()))
                        .collect::<Vec<_>>();
                    owners.sort_unstable();
                    design::assembly::placement_ordered_references_hold(scope, alignment, &owners)
                } else {
                    scope
                        .reference_members()
                        .values()
                        .rev()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value).rev())
                };
                let additional_members_link =
                    rigid_group_members_link(scope, alignment, rigid_as_built, stream_types);
                operand_frames_link
                    && alignment.additional_tail_reference.is_none()
                    && (alignment.additional_owner_run.is_none()
                        || design::assembly::placement_ordered_as_built_scope(scope))
                    && solved_frame_link
                    && operand_qualifiers_link
                    && additional_members_link
                    && joint_origin_envelope_link
                    && alignment_reference_link
                    && alignment_scalars_link
            }
        };
        let component_insert_link = match scope.component_insert_construction() {
            None => scope.component_insert_placement().is_none_or(|placement| {
                scope.reference_members().len() == 1
                    && match (
                        scope.canonical_name_frame_length(),
                        placement.matrix.as_ref(),
                    ) {
                        (Some(256 | 257 | 261 | 267 | 271 | 272 | 275 | 276), None) => true,
                        (Some(385), Some(matrix)) => {
                            scope.byte_offset().checked_add(u64_from_index(
                                crate::layout::component_insert_scope_283_262_385::TRANSFORM,
                            )) == Some(matrix.offset)
                        }
                        (Some(389 | 399), Some(matrix)) => scope
                            .byte_offset()
                            .checked_add(u64_from_index(
                            crate::layout::component_insert_matrix_scope_414_264_prefix::TRANSFORM,
                        )) == Some(matrix.offset),
                        (Some(frame @ (400 | 404)), Some(matrix)) => scope
                            .byte_offset()
                            .checked_add(
                            u64_from_index(
                                crate::layout::component_insert_matrix_scope_400_prefix::TRANSFORM,
                            ) + frame
                                - 400,
                        ) == Some(matrix.offset),
                        _ => false,
                    }
            }),
            Some(construction) => {
                let relation =
                    records_by_index.get(&(native_stream, construction.relation_record_index));
                let frame_matches_transform = match (
                    scope.canonical_name_frame_length().unwrap_or_default(),
                    scope.paired_class_tag.as_str(),
                    construction.placement.as_ref(),
                ) {
                    (271 | 272 | 275 | 276, _, None) => true,
                    (261, "263", None) if scope.class_tag.as_str() == "296" => true,
                    (261, "261", None) if scope.class_tag.as_str() == "410" => true,
                    (261, "258", None) if scope.class_tag.as_str() == "426" => true,
                    (261, "266", None) if scope.class_tag.as_str() == "434" => true,
                    (257 | 261 | 267, "264", None) if scope.class_tag.as_str() == "414" => true,
                    (257, "262", None) if scope.class_tag.as_str() == "283" => true,
                    (261, _, None)
                        if crate::records::feature::assembly_features::legacy_identity_insert_class(
                            scope.class_tag.as_str(),
                        ) =>
                    {
                        true
                    }
                    (385, "262", Some(matrix)) if scope.class_tag.as_str() == "283" => {
                        scope
                            .byte_offset()
                            .checked_add(46)
                            .is_some_and(|expected_offset| matrix.scope.offset == expected_offset)
                            && matrix.carrier_offset.is_none()
                    }
                    (frame_length @ (400 | 404), _, Some(matrix)) => {
                        scope
                            .byte_offset()
                            .checked_add(if frame_length == 400 { 50 } else { 54 })
                            .is_some_and(|expected_offset| matrix.scope.offset == expected_offset)
                            && matrix
                                .carrier_offset
                                .is_some_and(|offset| offset < construction.neutron_role_offset)
                    }
                    (frame_length, paired, Some(matrix)) => {
                        let scope_delta = match (frame_length, paired) {
                            (399, _) => Some(50),
                            (381, _) => Some(49),
                            (395, _) => Some(46),
                            (389, "264") if scope.class_tag.as_str() == "414" => Some(50),
                            (
                                crate::records::feature::assembly_features::TYPED_COMPONENT_INSERT_FRAME_LENGTH,
                                _,
                            ) => Some(u64_from_index(
                                crate::layout::component_insert_typed_matrix_scope_584_prefix::TRANSFORM,
                            )),
                            _ => None,
                        };
                        scope_delta.is_some_and(|delta| {
                            scope
                                .byte_offset()
                                .checked_add(delta)
                                .is_some_and(|expected_offset| {
                                    matrix.scope.offset == expected_offset
                                })
                        }) && matrix
                            .carrier_offset
                            .is_some_and(|offset| construction.neutron_role_offset < offset)
                    }
                    _ => false,
                };
                let role_valid = valid_component_insert_role(&construction.neutron_role);
                scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([construction.relation_record_index])
                    && construction.carrier_record_index != construction.relation_record_index
                    && role_valid
                    && frame_matches_transform
                    && relation.is_some_and(|relation| {
                        construction
                            .carrier_transform_offset()
                            .is_none_or(|offset| offset < relation.byte_offset)
                    })
                    && (native.xref_references.is_empty()
                        || crate::xref::component_insert_reference(
                            &native.xref_references,
                            native_stream,
                            construction,
                        )
                        .is_some())
            }
        };
        let copy_paste_component_link = match scope.copy_paste_component_operation() {
            None => true,
            Some(operation) => {
                let source = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.source_occurrence_record_index
                    });
                let copied = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.copied_occurrence_record_index
                    });
                let source_at = match scope.frame_length() {
                    529 => 38,
                    525 => 34,
                    _ => 0,
                };
                source_at != 0
                    && scope
                        .reference_members()
                        .values()
                        .copied()
                        .eq([operation.relation_record_index])
                    && operation.source_occurrence_record_index
                        != operation.copied_occurrence_record_index
                    && Some(operation.source_transform_offset)
                        == scope.byte_offset().checked_add(source_at)
                    && Some(operation.copied_transform_offset)
                        == scope.byte_offset().checked_add(source_at + 156)
                    && source.is_some_and(|source| {
                        source
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && source
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.source_occurrence_guid.as_str())
                            && source.transform().is_none()
                    })
                    && copied.is_some_and(|copied| {
                        copied
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && copied
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.copied_occurrence_guid.as_str())
                            && copied.transform().map(|frame| frame.value)
                                == Some(operation.copied_transform)
                    })
            }
        };
        let draft_link = match scope.draft_operation() {
            None => true,
            Some(operation) => {
                scope.reference_members().len() >= 6
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.angle_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.opposite_angle_record_index)
                    && operation.angle_record_index != operation.opposite_angle_record_index
                    && operation.angle_offset > scope.paired_byte_offset()
                    && operation.opposite_angle_offset > operation.angle_offset
                    && records_by_index.contains_key(&(native_stream, operation.angle_record_index))
                    && records_by_index
                        .contains_key(&(native_stream, operation.opposite_angle_record_index))
                    && operation
                        .flags
                        .and_then(|flags| flags.parting)
                        .is_none_or(|parting| {
                            [
                                Some(parting.pull),
                                Some(parting.tool),
                                Some(parting.faces),
                                parting.fixed_edges,
                            ]
                            .into_iter()
                            .flatten()
                            .all(|reference| {
                                reference.value != 0
                                    && reference.offset >= scope.byte_offset()
                                    && reference
                                        .offset
                                        .checked_add(11)
                                        .is_some_and(|end| end <= scope.reference_count_offset())
                                    && scope
                                        .reference_members()
                                        .values()
                                        .any(|value| *value == reference.value)
                                    && records_by_index
                                        .contains_key(&(native_stream, reference.value))
                            })
                        })
            }
        };
        let combine_link = match scope.combine_operation() {
            None => true,
            Some(operation) => {
                let expected_selections = scope
                    .reference_members()
                    .values()
                    .skip(1)
                    .step_by(2)
                    .copied()
                    .collect::<HashSet<_>>();
                let selections = std::iter::once(operation.target_record_index)
                    .chain(operation.tools.iter().map(|tool| tool.record_index))
                    .collect::<Vec<_>>();
                let actual_selections = selections.iter().copied().collect::<HashSet<_>>();
                let valid_external =
                    |selection: &records::feature::combine::DesignCombineBodySelection| {
                        selection.external_identity.as_ref().is_none_or(|identity| {
                            let Some(header) =
                                records_by_index.get(&(native_stream, selection.record_index))
                            else {
                                return false;
                            };
                            header.byte_offset.checked_add(44)
                                == Some(identity.selector_asset_id_offset())
                        })
                    };
                let compact_scope = scope.class_tag.as_str() == "387"
                    && scope.paired_class_tag.as_str() == "258"
                    && design::decode::scopes::parameter_scope::parameter_scope_payload_length(
                        scope,
                    ) == Some(314);
                let extended_reference_scope = scope.class_tag.as_str() == "329"
                    && scope.paired_class_tag.as_str() == "261"
                    && scope.frame_length() == 363;
                scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && selections.len() == scope.reference_members().len() / 2
                    && actual_selections.len() == selections.len()
                    && actual_selections == expected_selections
                    && operation.tools.iter().all(valid_external)
                    && match operation.form {
                        records::feature::combine::DesignCombineForm::Standard => {
                            !compact_scope
                                && !extended_reference_scope
                                && scope.byte_offset().checked_add(20).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(25).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::Compact => {
                            compact_scope
                                && scope.byte_offset().checked_add(21).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(25).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::OperandLists => {
                            !compact_scope
                                && !extended_reference_scope
                                && operand_list_combine_offsets_hold(scope.byte_offset(), operation)
                        }
                        records::feature::combine::DesignCombineForm::ExtendedReference => {
                            extended_reference_scope
                                && scope.byte_offset().checked_add(31).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(30).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                    }
            }
        };
        let thread_link = match scope.thread_construction() {
            None => true,
            Some(construction) => {
                let references = scope
                    .reference_members()
                    .values()
                    .copied()
                    .collect::<Vec<_>>();
                let expected_groups = if construction.form
                    == records::feature::thread::DesignThreadForm::StandardExtended
                {
                    construction
                        .form
                        .owner_ordinals(scope, &native.design_parameter_owners)
                        .map(|ordinals| {
                            records::feature::thread::DesignThreadForm::extended_standard_face_groups(
                                &references,
                                ordinals,
                            )
                        })
                        .unwrap_or_default()
                } else {
                    construction.form.face_groups(&references)
                };
                construction
                    .form
                    .owners_hold(scope, &native.design_parameter_owners)
                    && construction
                        .form
                        .reference_count_holds(scope.reference_members().len())
                    && thread_class_pair_holds(
                        construction.form,
                        &scope.class_tag,
                        &scope.paired_class_tag,
                    )
                    && construction.face_group_record_indices == expected_groups
                    && thread_designation_offset_holds(
                        construction.form,
                        construction.designation_offset,
                        scope.byte_offset(),
                    )
                    && match construction.form {
                        records::feature::thread::DesignThreadForm::Compact(Some(reference))
                        | records::feature::thread::DesignThreadForm::CompactExtended(reference) => {
                            reference.offset > construction.designation_offset
                                && reference.offset < scope.paired_byte_offset()
                                && records_by_index
                                    .contains_key(&(native_stream, reference.value.get()))
                        }
                        records::feature::thread::DesignThreadForm::Compact(None)
                        | records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::StandardLegacy
                        | records::feature::thread::DesignThreadForm::StandardExtended
                        | records::feature::thread::DesignThreadForm::StandardCompact
                        | records::feature::thread::DesignThreadForm::CompactLegacy
                        | records::feature::thread::DesignThreadForm::CompactOwnersLeading
                        | records::feature::thread::DesignThreadForm::CompactOwnersTrailing => true,
                    }
                    && construction
                        .face_group_record_indices
                        .iter()
                        .enumerate()
                        .all(|(group_ordinal, record_index)| {
                            let compact_member = if matches!(
                                construction.form,
                                records::feature::thread::DesignThreadForm::Compact(_)
                                    | records::feature::thread::DesignThreadForm::CompactExtended(
                                        _
                                    )
                                    | records::feature::thread::DesignThreadForm::CompactLegacy
                                    | records::feature::thread::DesignThreadForm::CompactOwnersLeading
                                    | records::feature::thread::DesignThreadForm::CompactOwnersTrailing
                            ) {
                                let Some(reference_ordinal) =
                                    group_ordinal.checked_mul(2).and_then(|ordinal| {
                                        ordinal
                                            .checked_add(construction.form.face_reference_start())
                                    })
                                else {
                                    return false;
                                };
                                let Some(member_ordinal) = reference_ordinal.checked_add(1) else {
                                    return false;
                                };
                                let Some(member_record_index) =
                                    scope.reference_members().values().nth(member_ordinal)
                                else {
                                    return false;
                                };
                                let Ok(scope_reference_ordinal) = u32::try_from(reference_ordinal)
                                else {
                                    return false;
                                };
                                Some((scope_reference_ordinal, *member_record_index))
                            } else {
                                None
                            };
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == *record_index
                                        && group.role() == DesignOperandRole::ROLE_0X10
                                        && compact_member.is_none_or(
                                            |(scope_reference_ordinal, member_record_index)| {
                                                group.scope_reference_ordinal
                                                    == scope_reference_ordinal
                                                    && group
                                                        .members()
                                                        .iter()
                                                        .map(|member| member.value)
                                                        .eq([member_record_index])
                                            },
                                        )
                                });
                            groups.next().is_some() && groups.next().is_none()
                        })
            }
        };
        let joint_origin_link = scope.joint_origin_frame().is_none_or(|origin| {
            if origin.carrier.is_some() {
                return direct_joint_origin_carrier_link(scope, origin);
            }
            let transform = origin.joint_origin_transform;
            let transform_offset = origin.joint_origin_transform_offset;
            let inline = match (scope.frame_length(), &origin.reference) {
                (385, None) => Some(transform_offset) == scope.byte_offset().checked_add(49),
                (336 | 347, Some(reference)) => {
                    Some(transform_offset) == scope.byte_offset().checked_add(60)
                        && Some(reference.joint_origin_reference_offset)
                            == scope.byte_offset().checked_add(46)
                        && scope
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                }
                _ => false,
            };
            let assembly_operand = origin.reference.is_none()
                && native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.assembly_alignment().is_some_and(|alignment| {
                            alignment.operand_frames().is_some_and(|frames| {
                                frames.iter().any(|frame| {
                                    frame.reference_record_index == scope.record_index
                                        && frame.transform == transform
                                        && frame.transform_offset == transform_offset
                                })
                            })
                        })
                });
            let single_operand_assembly = origin.reference.as_ref().is_some_and(|reference| {
                native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.class_tag.as_str() == "276"
                        && assembly.paired_class_tag.as_str() == "258"
                        && assembly.frame_length() == 604
                        && Some(transform_offset) == assembly.byte_offset().checked_add(36)
                        && assembly
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                        && Some(reference.joint_origin_reference_offset)
                            == assembly.byte_offset().checked_add(25)
                })
            });
            inline || assembly_operand || single_operand_assembly
        });
        let work_point_link = valid_work_point_construction(ctx, scope, native_stream)?;
        let work_plane_link = valid_work_plane_construction(ctx, scope, native_stream);
        let record_offset = |index| {
            records_by_index
                .get(&(native_stream, index))
                .map(|header| header.byte_offset)
        };
        let before_scope =
            |index| record_offset(index).is_some_and(|offset| offset < scope.byte_offset());
        let before = |left, right| {
            record_offset(left)
                .zip(record_offset(right))
                .is_some_and(|(left, right)| left < right)
        };
        let has_type = |index, guid: &str| {
            native
                .design_types
                .iter()
                .filter(|kind| {
                    ids::design_segment(kind.id()) == ids::design_segment(&scope.id)
                        && kind.type_guid.as_str().eq_ignore_ascii_case(guid)
                        && kind
                            .entities
                            .values()
                            .any(|entity| *entity == u64::from(index))
                })
                .count()
                == 1
        };
        let valid = match scope.extrude_prologue() {
            Some(_) if scope.is_legacy_class_330_blind_cut() => true,
            Some(prologue) if through_all::matches_scope(scope) => {
                through_all::valid_prologue(scope, prologue)
            }
            Some(records::feature::extrude::DesignExtrudePrologue::LegacyDistance {
                prefix_zero_offset,
                operation_offset,
                extent_kind_offset,
                direction_reversed_offset,
                solid_operation_offset,
                ..
            }) => {
                let marker_offset = scope.byte_offset().checked_add(20);
                let prefix_valid =
                    match prefix_zero_offset {
                        None => {
                            marker_offset
                                .and_then(|offset| offset.checked_add(1))
                                .is_some_and(|expected_offset| operation_offset == expected_offset)
                                && scope.byte_offset().checked_add(208).is_some_and(
                                    |expected_offset| {
                                        scope.reference_count_offset() == expected_offset
                                    },
                                )
                        }
                        Some(offset) => {
                            marker_offset
                                .and_then(|offset| offset.checked_add(1))
                                .is_some_and(|expected_offset| offset == expected_offset)
                                && offset.checked_add(4).is_some_and(|expected_offset| {
                                    operation_offset == expected_offset
                                })
                                && scope.byte_offset().checked_add(212).is_some_and(
                                    |expected_offset| {
                                        scope.reference_count_offset() == expected_offset
                                    },
                                )
                        }
                    };
                prefix_valid
                    && operation_offset
                        .checked_add(4)
                        .is_some_and(|expected_offset| extent_kind_offset == expected_offset)
                    && extent_kind_offset
                        .checked_add(4)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && direction_reversed_offset
                        .checked_add(1)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
            }
            Some(records::feature::extrude::DesignExtrudePrologue::ShiftedReferenceAware {
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let expected_layout =
                    match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
                        ("357", "258") | ("275" | "361", "262") => Some((
                            538_u64,
                            292_u64,
                            13_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        )),
                        ("349", "266") => Some((
                            538_u64,
                            292_u64,
                            13_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        )),
                        ("323", "263")
                            if scope.byte_offset().checked_add(292).is_some_and(
                                |expected_offset| scope.reference_count_offset() == expected_offset,
                            ) =>
                        {
                            Some((
                                516_u64,
                                292_u64,
                                11_usize,
                                [2, 1],
                                [2, 0],
                                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                                288_u64,
                            ))
                        }
                        ("323", "263")
                            if scope.byte_offset().checked_add(272).is_some_and(
                                |expected_offset| scope.reference_count_offset() == expected_offset,
                            ) =>
                        {
                            Some((
                                485_u64,
                                272_u64,
                                10_usize,
                                [3, 0],
                                [4, 4],
                                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll,
                                129_u64,
                            ))
                        }
                        _ => None,
                    };
                expected_layout.is_some_and(
                    |(
                        frame_length,
                        reference_count_offset,
                        reference_member_count,
                        expected_direction_face_extend_values,
                        expected_side_extent_discriminators,
                        expected_extent,
                        second_side_extent_offset,
                    )| {
                        scope.frame_length() == frame_length
                            && scope.byte_offset().checked_add(frame_length).is_some_and(
                                |expected_offset| scope.paired_byte_offset() == expected_offset,
                            )
                            && scope
                                .byte_offset()
                                .checked_add(reference_count_offset)
                                .is_some_and(|expected_offset| {
                                    scope.reference_count_offset() == expected_offset
                                })
                            && scope.reference_members().len() == reference_member_count
                            && scope
                                .byte_offset()
                                .checked_add(27)
                                .is_some_and(|expected_offset| operation_offset == expected_offset)
                            && direction_face_extend_values == expected_direction_face_extend_values
                            && side_extent_discriminators == expected_side_extent_discriminators
                            && extent == expected_extent
                            && scope
                                .byte_offset()
                                .checked_add(116)
                                .zip(scope.byte_offset().checked_add(second_side_extent_offset))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(31)
                                .zip(scope.byte_offset().checked_add(35))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    direction_face_extend_offsets == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(39)
                                .is_some_and(|expected_offset| {
                                    direction_reversed_offset == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(40)
                                .is_some_and(|expected_offset| {
                                    solid_operation_offset == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(41)
                                .is_some_and(|expected_offset| start_offset == expected_offset)
                    },
                )
            }
            Some(records::feature::extrude::DesignExtrudePrologue::ReferenceAware {
                reference,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                opaque_distance_word,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                solid_operation,
                start_offset,
                start,
                ..
            }) => {
                let typed_references = scope_has_typed_references(scope);
                let prefix_valid = reference.map_or(
                    direct_extrude_operation_valid(
                        scope.byte_offset(),
                        typed_references,
                        operation_offset,
                    ),
                    |reference| {
                        let padding_end =
                            reference
                                .record_index_offset
                                .checked_add(4)
                                .and_then(|offset| {
                                    offset.checked_add(u64::from(reference.trailing_zero_count))
                                });
                        let marker_valid = match reference.operation_prefix_marker_offset {
                            None => Some(operation_offset) == padding_end,
                            Some(marker_offset) => {
                                Some(marker_offset) == padding_end
                                    && marker_offset.checked_add(1).is_some_and(|expected_offset| {
                                        operation_offset == expected_offset
                                    })
                            }
                        };
                        let encoding_valid = if reference.type_guid.is_some() {
                            typed_references
                                && reference.trailing_zero_count == 0
                                && reference.operation_prefix_marker_offset.is_none()
                                && scope.byte_offset().checked_add(u64_from_index(
                                    crate::layout::typed_reference_extrude_anchor_prefix::LEN,
                                )) == Some(operation_offset)
                        } else {
                            matches!(reference.trailing_zero_count, 6..=8) && marker_valid
                        };
                        scope
                            .byte_offset()
                            .checked_add(26)
                            .is_some_and(|expected_offset| {
                                reference.record_index_offset == expected_offset
                            })
                            && encoding_valid
                            && scope
                                .reference_members()
                                .values()
                                .any(|value| value == &reference.record_index)
                    },
                );
                let target_ordinal_valid = first_side_target_ordinal.is_none_or(|target| {
                    usize::try_from(target.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| {
                            scope.reference_members().values().nth(ordinal).copied()
                        })
                        .is_some_and(|record_index| {
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == record_index
                                        && group.scope_reference_ordinal
                                            == target.scope_reference_ordinal
                                        && group.role() == DesignOperandRole::ROLE_0X5
                                        && group.extrude_role().is_none()
                                });
                            target.scope_reference_ordinal_offset.checked_add(5)
                                == Some(side_extent_discriminator_offsets[0])
                                && groups.next().is_some()
                                && groups.next().is_none()
                        })
                });
                let target_prefix_length = if first_side_target_ordinal.is_some() {
                    5
                } else {
                    0
                };
                let legacy_class_415_layout = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_delta| {
                        legacy_class_415::is_symmetric_distance_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_delta,
                            scope.reference_members().len(),
                        )
                    });
                let legacy_class_415_one_sided_layout = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_delta| {
                        legacy_class_415::is_one_sided_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_delta,
                            scope.reference_members().len(),
                        )
                    });
                let legacy_class_415_extent = legacy_class_415_layout
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                    && direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 1]
                    && extent == records::feature::extrude::DesignExtrudeExtent::SymmetricDistance
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_415::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::DIRECTION))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_415::FACE_EXTEND)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::DIRECTION_REVERSED))
                        .is_some_and(|expected_offset| {
                            direction_reversed_offset == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::GEOMETRY_KIND))
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::START_SUPPORT))
                        .is_some_and(|expected_offset| start_offset == expected_offset);
                let first_side_offset_valid = operation_offset
                    .checked_add(49)
                    .and_then(|offset| offset.checked_add(target_prefix_length))
                    .and_then(|offset| side_extent_discriminator_offsets[0].checked_sub(offset))
                    .is_some_and(|slot_expansion| {
                        extent_slot_expansion_valid(typed_references, slot_expansion)
                    });
                let opaque_distance_word_valid = opaque_distance_word.is_none_or(|word| {
                    typed_references
                        && scope.byte_offset().checked_add(u64_from_index(
                            crate::layout::typed_reference_extrude_prologue::OPERATION,
                        )) == Some(operation_offset)
                        && direction_face_extend_values == [1, 2]
                        && side_extent_discriminators == [1, 0]
                        && solid_operation
                        && start == records::feature::extrude::DesignExtrudeStart::ProfilePlane
                        && side_extent_discriminator_offsets[0].checked_add(u64_from_index(
                            crate::layout::current_extrude_non_target_extent_pair::TARGET_OPTION,
                        )) == Some(word.byte_offset)
                });
                let second_side_offset_valid = (if side_extent_discriminators == [2, 2]
                    || (side_extent_discriminators == [2, 1]
                        && extent == records::feature::extrude::DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero)
                {
                    side_extent_discriminator_offsets[0].checked_add(u64_from_index(
                        crate::layout::current_extrude_face_target_payload::SECOND_SIDE_EXTENT,
                    ))
                } else if side_extent_discriminators[0] == 2 {
                    scope.reference_count_offset().checked_sub(4)
                } else {
                    side_extent_discriminator_offsets[0].checked_add(13)
                })
                .is_some_and(|expected_offset| {
                    side_extent_discriminator_offsets[1] == expected_offset
                }) || (legacy_class_415_one_sided_layout
                    && scope.reference_count_offset().checked_sub(4).is_some_and(
                        |expected_offset| side_extent_discriminator_offsets[1] == expected_offset,
                    ));
                let standard_extent = design::decode::scopes::extrude::exact_extrude_extent(
                    direction_face_extend_values[0],
                    side_extent_discriminators,
                ) == Some(extent);
                let mixed_extent = direction_face_extend_values[0] == 2
                    && matches!(
                        (side_extent_discriminators, extent),
                        ([1, 2], records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
                            | records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne)
                            | ([4, 2], records::feature::extrude::DesignExtrudeExtent::TwoSidedThroughAllToFace)
                            | ([2, 1], records::feature::extrude::DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero)
                            | ([2, 2], records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionZero
                                | records::feature::extrude::DesignExtrudeExtent::TwoSidedToTargetsOptionTwo)
                    );
                let dual_lane_symmetric_extent = direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 1]
                    && extent == records::feature::extrude::DesignExtrudeExtent::SymmetricDistance;
                prefix_valid
                    && matches!(direction_face_extend_values[0], 1..=3)
                    && (standard_extent
                        || mixed_extent
                        || legacy_class_415_extent
                        || dual_lane_symmetric_extent)
                    && first_side_target_ordinal.is_none_or(|_| side_extent_discriminators[0] == 2)
                    && target_ordinal_valid
                    && opaque_distance_word_valid
                    && first_side_offset_valid
                    && second_side_offset_valid
                    && operation_offset
                        .checked_add(4)
                        .zip(operation_offset.checked_add(8))
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && operation_offset
                        .checked_add(14)
                        .is_some_and(|expected_offset| start_offset == expected_offset)
                    && operation_offset
                        .checked_add(13)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && operation_offset
                        .checked_add(12)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && side_extent_discriminator_offsets[1]
                        .checked_add(4)
                        .is_some_and(|end| end <= scope.reference_count_offset())
            }
            Some(records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let field_shift =
                    match operation_prefix_marker_offset {
                        None if scope
                            .byte_offset()
                            .checked_add(27)
                            .is_some_and(|expected_offset| operation_offset == expected_offset) =>
                        {
                            Some(0)
                        }
                        Some(marker_offset)
                            if scope.byte_offset().checked_add(27).is_some_and(
                                |expected_offset| marker_offset == expected_offset,
                            ) && marker_offset.checked_add(1).is_some_and(
                                |expected_offset| operation_offset == expected_offset,
                            ) =>
                        {
                            Some(1)
                        }
                        _ => None,
                    };
                let compact_extent_offsets = if operation_prefix_marker_offset.is_none()
                    && scope
                        .byte_offset()
                        .checked_add(26)
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .and_then(|offset| match offset {
                            251 => scope
                                .byte_offset()
                                .checked_add(105)
                                .zip(scope.byte_offset().checked_add(109))
                                .map(|(first, second)| [first, second]),
                            281 => scope
                                .byte_offset()
                                .checked_add(124)
                                .zip(scope.byte_offset().checked_add(128))
                                .map(|(first, second)| [first, second]),
                            _ => None,
                        })
                } else {
                    None
                };
                let class_296_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_one_sided_to_face_layout(
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_to_face::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_to_face::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_296_to_face::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_296_symmetric_extent_offsets =
                    if scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .is_some_and(|reference_count_offset| {
                            is_class_296_symmetric_distance_layout(
                                scope.class_tag.as_str(),
                                scope.paired_class_tag.as_str(),
                                scope.frame_length(),
                                reference_count_offset,
                                scope.reference_members().len(),
                            )
                        })
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                    {
                        scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::FIRST_SIDE_EXTENT))
                            .zip(scope.byte_offset().checked_add(u64_from_index(
                                class_296_symmetric::SECOND_SIDE_EXTENT,
                            )))
                            .map(|(first, second)| [first, second])
                    } else {
                        None
                    };
                let class_296_two_faces_extent_offsets =
                    if scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .is_some_and(|reference_count_offset| {
                            is_class_296_two_sided_to_faces_layout(
                                scope.class_tag.as_str(),
                                scope.paired_class_tag.as_str(),
                                scope.frame_length(),
                                reference_count_offset,
                                scope.reference_members().len(),
                            )
                        })
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                    {
                        scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::FIRST_SIDE_EXTENT))
                            .zip(scope.byte_offset().checked_add(u64_from_index(
                                class_296_two_faces::SECOND_SIDE_EXTENT,
                            )))
                            .map(|(first, second)| [first, second])
                    } else {
                        None
                    };
                let class_296_legacy_to_face_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_to_face_layout(
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(scope.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_to_face::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_296_legacy_distance_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_distance_layout(
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(scope.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_distance::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_397_frame = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .and_then(|reference_count_offset| {
                        Class397SymmetricFrame::new(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    });
                let extent_valid = if let Some(frame) = class_397_frame {
                    operation_prefix_marker_offset.is_none()
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_397::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                        && direction_face_extend_values
                            == [class_397::DIRECTION_VALUE, class_397::FACE_EXTEND_VALUE]
                        && extent.is_some()
                        && extent
                            == frame
                                .extent(direction_face_extend_values[0], side_extent_discriminators)
                } else if compact_extent_offsets.is_some() {
                    matches!(
                        (
                            direction_face_extend_values,
                            side_extent_discriminators,
                            extent,
                        ),
                        (
                            [1, _],
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                        ) | (
                            [3, _],
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                        ) | (
                            [2, 0],
                            [1, 2],
                            Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace)
                        )
                    )
                } else if class_296_extent_offsets.is_some() {
                    direction_face_extend_values[0] == 1
                        && matches!(direction_face_extend_values[1], 1 | 2)
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                } else if class_296_symmetric_extent_offsets.is_some() {
                    direction_face_extend_values == [3, 2]
                        && side_extent_discriminators == [1, 0]
                        && extent
                            == Some(
                                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance,
                            )
                } else if class_296_two_faces_extent_offsets.is_some() {
                    direction_face_extend_values[0] == 2
                        && matches!(direction_face_extend_values[1], 1 | 2)
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces)
                } else if class_296_legacy_to_face_extent_offsets.is_some() {
                    direction_face_extend_values == [1, 1]
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                } else if class_296_legacy_distance_extent_offsets.is_some() {
                    direction_face_extend_values == [1, 2]
                        && side_extent_discriminators == [1, 0]
                        && extent
                            == Some(
                                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance,
                            )
                } else {
                    matches!(direction_face_extend_values[0], 1..=3)
                        && matches!(
                            (
                                direction_face_extend_values[0],
                                side_extent_discriminators,
                                extent,
                            ),
                            (
                                1,
                                [1, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                            ) | (
                                1,
                                [2, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                            ) | (
                                1,
                                [3, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext),
                            ) | (
                                1,
                                [4, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll)
                            ) | (
                                2,
                                [1, 1],
                                Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance)
                            ) | (
                                3,
                                [1, 0 | 1],
                                Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                            ) | (
                                3,
                                [4, 4],
                                Some(records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll)
                            )
                        )
                };
                let side_offsets_valid = if class_397_frame.is_some() {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_397::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_397::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                } else {
                    compact_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_symmetric_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_two_faces_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_legacy_to_face_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_legacy_distance_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || field_shift.is_some_and(|field_shift| {
                            (if direction_face_extend_values[0] == 2 {
                                if scope
                                    .reference_count_offset()
                                    .checked_sub(scope.byte_offset())
                                    .and_then(|offset| offset.checked_sub(field_shift))
                                    == Some(283)
                                {
                                    scope
                                        .byte_offset()
                                        .checked_add(166 + field_shift)
                                        .zip(scope.byte_offset().checked_add(181 + field_shift))
                                        .map(|(first, second)| [first, second])
                                } else {
                                    scope
                                        .byte_offset()
                                        .checked_add(155 + field_shift)
                                        .zip(scope.byte_offset().checked_add(178 + field_shift))
                                        .map(|(first, second)| [first, second])
                                }
                            } else if side_extent_discriminators[0] == 2 {
                                let first_offset = side_extent_discriminator_offsets[0];
                                if matches!(
                                    first_offset
                                        .checked_sub(scope.byte_offset())
                                        .and_then(|offset| offset.checked_sub(field_shift)),
                                    Some(106 | 116)
                                ) {
                                    scope
                                        .reference_count_offset()
                                        .checked_sub(4)
                                        .map(|second| [first_offset, second])
                                } else {
                                    Some([0, 0])
                                }
                            } else if scope
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .zip(scope.byte_offset().checked_add(129 + field_shift))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets == expected_offset
                                })
                            {
                                Some(side_extent_discriminator_offsets)
                            } else if scope
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets[0] == expected_offset
                                })
                            {
                                scope
                                    .byte_offset()
                                    .checked_add(116 + field_shift)
                                    .zip(scope.byte_offset().checked_add(130 + field_shift))
                                    .map(|(first, second)| [first, second])
                            } else {
                                scope
                                    .byte_offset()
                                    .checked_add(106 + field_shift)
                                    .zip(scope.byte_offset().checked_add(110 + field_shift))
                                    .map(|(first, second)| [first, second])
                            })
                            .is_some_and(|expected_offset| {
                                side_extent_discriminator_offsets == expected_offset
                            })
                        })
                };
                (field_shift.is_some()
                    || compact_extent_offsets.is_some()
                    || class_296_extent_offsets.is_some()
                    || class_296_symmetric_extent_offsets.is_some()
                    || class_296_two_faces_extent_offsets.is_some()
                    || class_296_legacy_to_face_extent_offsets.is_some()
                    || class_296_legacy_distance_extent_offsets.is_some())
                    && extent_valid
                    && side_offsets_valid
                    && operation_offset
                        .checked_add(4)
                        .zip(operation_offset.checked_add(8))
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && operation_offset
                        .checked_add(14)
                        .is_some_and(|expected_offset| start_offset == expected_offset)
                    && operation_offset
                        .checked_add(13)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && operation_offset
                        .checked_add(12)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && direction_face_extend_offsets[1] < scope.reference_count_offset()
            }
            None => true,
        } && match &scope.payload() {
            records::feature::scope::DesignScopePayload::SurfaceStitch(operation) => {
                operation.gap_tolerance_offset > scope.paired_byte_offset()
                    && scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && scope.reference_members().values().rev().nth(1)
                        == Some(&operation.tolerance_record_index)
                    && scope.reference_members().values().next_back()
                        == Some(&operation.settings_record_index)
            }
            _ => true,
        } && match &scope.payload() {
            records::feature::scope::DesignScopePayload::SurfaceRuled(operation) => {
                scope
                    .byte_offset()
                    .checked_add(20)
                    .is_some_and(|expected_offset| operation.method_offset == expected_offset)
                    && scope
                        .byte_offset()
                        .checked_add(27)
                        .is_some_and(|expected_offset| {
                            operation.alternate_face_offset == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(50)
                        .is_some_and(|expected_offset| operation.corner_offset == expected_offset)
                    && scope.reference_members().values().next()
                        == Some(&operation.distance_owner_record_index)
                    && scope.reference_members().values().nth(1)
                        == Some(&operation.angle_owner_record_index)
                    && operation.distance_owner_record_index != operation.angle_owner_record_index
                    && !operation.edge_group_record_indices.is_empty()
                    && operation
                        .edge_group_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                        })
                    && match operation.method {
                        records::feature::surface_ops::DesignRuledSurfaceMethod::Direction => {
                            operation.direction_entity_id.is_some()
                        }
                        records::feature::surface_ops::DesignRuledSurfaceMethod::Normal
                        | records::feature::surface_ops::DesignRuledSurfaceMethod::Tangent => {
                            operation.direction_entity_id.is_none()
                        }
                    }
            }
            _ => true,
        } && deleted_members_hold(scope, records_by_index, native_stream)
            && scope.reference_members().values().all(|record_index| {
                records_by_index.contains_key(&(native_stream, *record_index))
                    || scope.deleted_members.as_ref().is_some_and(|deleted| {
                        deleted.members.iter().any(|member| member == record_index)
                    })
            })
            && records_by_index.contains_key(&(native_stream, scope.record_index))
            && entity_link.unwrap_or(true)
            && extrude_profile_link
            && sweep_profile_link
            && base_flange_profile_link
            && emboss_profile_link
            && move_link
            && offset_faces_link
            && split_face_link
            && base_flange_link
            && fold_links
            && edge_flange_link
            && hem_link
            && copy_paste_link
            && rectangular_pattern_link
            && assembly_alignment_link
            && scope.align_envelope().is_none_or(|envelope| {
                design::decode::scopes::align::align_envelope_links_hold(scope, envelope)
                    && envelope.construction.as_ref().is_none_or(|construction| {
                        records_by_index
                            .get(&(native_stream, construction.record_index))
                            .is_some_and(|header| {
                                header.byte_offset == construction.byte_offset
                                    && header.class_tag == construction.class_tag
                            })
                    })
            })
            && component_insert_link
            && copy_paste_component_link
            && scope.occurrence_transfer.as_ref().is_none_or(|operation| {
                operation.links_valid(scope, record_offset, |index, guid| has_type(index, guid))
            })
            && scope
                .occurrence_transfer_intent
                .as_ref()
                .is_none_or(|intent| {
                    intent.links_valid(scope, before_scope, before, |index, guid| {
                        has_type(index, guid)
                    })
                })
            && draft_link
            && combine_link
            && thread_link
            && joint_origin_link
            && work_point_link
            && work_plane_link
            && super::sketch_scope::placement_link(ctx, scope)
            && unique_index;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter scope has an invalid paired frame",
                Some(scope.id.clone()),
            );
        }
        if split_face_link
            && scope.split_face_operation().is_some_and(
                crate::records::feature::split_face::DesignSplitFaceOperation::carrier_type_assumed,
            )
        {
            findings.push(Finding {
                rule: cadmpeg_ir::rule_name!(
                    "f3d/parameter_scopes.fusion-splitface-carrier-registration-conflicts-inline"
                ),
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Warning,
                message:
                    "Fusion SplitFace carrier registration conflicts with its inline type qualifier"
                        .into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
    Ok(())
}

/// Whether a scope's deleted-member evidence names a held record-timestamp
/// table owned by the scope whose stored bytes state its header, entries and
/// owner, and every listed member is a scope reference whose record index has
/// no indexed header.
pub(super) fn deleted_members_hold(
    scope: &records::feature::scope::DesignParameterScope,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    native_stream: &str,
) -> bool {
    scope.deleted_members.as_ref().is_none_or(|deleted| {
        deleted.frame_holds(scope.record_index)
            && design::decode::deleted_members::table_evidence(
                &deleted.table_bytes,
                deleted.table_byte_offset,
            )
            .is_some_and(|table| {
                table.record_index == deleted.table_record_index
                    && table.class_tag == deleted.table_class_tag
                    && table.entries == deleted.entries
                    && table.owner == deleted.owner
            })
            && records_by_index
                .get(&(native_stream, deleted.table_record_index))
                .is_some_and(|header| {
                    header.byte_offset == deleted.table_byte_offset
                        && header.class_tag == deleted.table_class_tag
                })
            && deleted.members.iter().all(|member| {
                !records_by_index.contains_key(&(native_stream, *member))
                    && scope
                        .reference_members()
                        .values()
                        .any(|record_index| record_index == member)
            })
    })
}

/// Entity of the document-root component in a Design that numbers it 3.
pub(super) const STANDARD_ROOT_COMPONENT_ENTITY: u32 = 3;
/// Type GUID of the Design component entities.
pub(super) const COMPONENT_TYPE_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";

/// Whether an ACT component link tracks the document-root component. The
/// root is entity 3 of the component type. A Design whose component type has
/// no entity 3 (a component saved as its own document) has its root link name
/// another entity of that type.
pub(super) fn act_link_is_document_root(
    native: &crate::native::F3dNative,
    stream: &str,
    tracked: u32,
) -> bool {
    if tracked == STANDARD_ROOT_COMPONENT_ENTITY {
        return true;
    }
    let folder = stream.rsplitn(3, '/').nth(2);
    let component_entities = |entity: u32| {
        native.design_types.iter().any(|kind| {
            kind.type_guid
                .as_str()
                .eq_ignore_ascii_case(COMPONENT_TYPE_GUID)
                && ids::native_stream(kind.id()).and_then(|design| design.rsplitn(3, '/').nth(2))
                    == folder
                && kind
                    .entities
                    .values()
                    .any(|value| *value == u64::from(entity))
        })
    };
    !component_entities(STANDARD_ROOT_COMPONENT_ENTITY) && component_entities(tracked)
}

/// Whether a scope stores 51-byte typed reference-table entries: the last
/// entry's target is followed by the 7-byte entry tail and the kind string.
pub(super) fn scope_has_typed_references(
    scope: &records::feature::scope::DesignParameterScope,
) -> bool {
    scope
        .reference_members()
        .offsets()
        .last()
        .and_then(|last| scope.kind_offset().checked_sub(*last))
        == Some(u64_from_index(crate::bytes::TYPED_LOCAL_REFERENCE_LEN) + 7)
}

/// Whether a reference-aware Extrude scope without a prologue reference
/// starts its operation fields at an admitted offset. The plain form starts
/// them at offset 24 or 28; a typed-reference scope starts them at offset 22
/// or 26.
pub(super) fn direct_extrude_operation_valid(
    scope_offset: u64,
    typed_references: bool,
    operation_offset: u64,
) -> bool {
    let admitted: &[usize] = if typed_references {
        &[
            crate::layout::typed_reference_extrude_prologue::OPERATION,
            crate::layout::shifted_typed_reference_extrude_prologue::OPERATION,
        ]
    } else {
        &[
            crate::layout::offset_23_extrude_prologue::OPERATION,
            crate::layout::offset_24_extrude_prologue::OPERATION,
            crate::layout::compact_shifted_extrude_prologue::OPERATION,
            crate::layout::shifted_extrude_prologue::OPERATION,
            28,
        ]
    };
    admitted
        .iter()
        .any(|delta| scope_offset.checked_add(u64_from_index(*delta)) == Some(operation_offset))
}

/// Whether the span between the empty-slot end of the operation fields and
/// the first side extent holds whole present extent slots. A present slot
/// replaces its one-byte absent marker with a reference, which widens the
/// span by 10 bytes (plain) or 50 bytes (typed) for each of the seven slots.
pub(super) fn extent_slot_expansion_valid(typed_references: bool, slot_expansion: u64) -> bool {
    let unit = if typed_references { 50 } else { 10 };
    slot_expansion <= 7 * unit && slot_expansion.is_multiple_of(unit)
}
