// SPDX-License-Identifier: Apache-2.0
//! Parse parameter scopes and exact feature-construction frames.

use cadmpeg_core::container::ContainerRole;

use crate::bytes::{f64s_at, is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded, take_reference};
use crate::container::ContainerScan;
use crate::design::decode::assembly::{
    exact_legacy_as_built_421_alignment, exact_legacy_as_built_421_operands,
    exact_legacy_as_built_421_solved_frame,
};
use crate::design::decode::operands::{
    parse_construction_operand_group, parse_entity_selection_frame, parse_entity_selection_prefix,
    parse_face_operand, ConstructionOperandGroupParse,
};
use crate::design::decode::sketch::{
    identity_matrix, next_indexed_record_offset, valid_sketch_transform, IndexedRecordOffsets,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::layout::assembly_axial_construction_carrier as axial_carrier;
use crate::layout::assembly_axial_role_prefix as axial_role;
use crate::layout::assembly_axial_selector_prefix as axial_selector;
use crate::layout::assembly_class_363_264_frame_360_child as class_363_child;
use crate::layout::assembly_class_363_264_frame_360_leading as class_363_leading;
use crate::layout::assembly_class_363_264_frame_363_carrier as class_363_carrier;
use crate::layout::assembly_class_363_264_frame_386_terminal as class_363_terminal;
use crate::layout::assembly_class_363_264_frame_388_identity as class_363_identity;
use crate::layout::assembly_class_363_264_frame_388_identity_extended as class_363_identity_extended;
use crate::layout::assembly_class_363_264_frame_388_identity_reduced_490 as class_363_identity_reduced_490;
use crate::layout::assembly_class_363_264_frame_388_identity_reduced_501 as class_363_identity_reduced_501;
use crate::layout::assembly_class_363_264_frame_388_identity_short as class_363_identity_short;
use crate::layout::assembly_class_383_258_frame_359_identity as class_383_identity;
use crate::layout::assembly_class_383_258_frame_378_carrier as class_383_carrier;
use crate::layout::assembly_class_383_258_frame_387_child as class_383_child;
use crate::layout::assembly_class_383_258_frame_387_leading as class_383_leading;
use crate::layout::assembly_class_383_258_frame_394 as class_383_face;
use crate::layout::assembly_class_383_258_scope_1011 as class_383_scope;
use crate::layout::assembly_class_388_266_scope_968 as class_388_assemble;
use crate::layout::assembly_class_406_261_scope_671 as class_406_assemble;
use crate::layout::assembly_legacy_class_369_path_wrapper_one as class_369_wrapper_one;
use crate::layout::assembly_legacy_class_369_path_wrapper_two as class_369_wrapper_two;
use crate::layout::assembly_legacy_class_412_path_425 as class_412_path;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_locator_reference_run as path_locator_run;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::layout::base_feature_body_snapshot_body_entry as snapshot_entry;
use crate::layout::base_feature_body_snapshot_compact_preamble as snapshot_compact_preamble;
use crate::layout::base_feature_body_snapshot_expanded_preamble as snapshot_expanded_preamble;
use crate::layout::base_feature_body_snapshot_guid as snapshot_guid;
use crate::layout::base_feature_body_snapshot_linkage_tail as snapshot_tail;
use crate::layout::base_feature_body_snapshot_prefix as snapshot;
use crate::layout::base_feature_body_snapshot_scope_prefix as snapshot_scope;
use crate::layout::base_feature_compact_repeated_body_entry as compact_entry;
use crate::layout::base_feature_compact_result_body_count as compact_count;
use crate::layout::base_feature_legacy_444_zero_body as legacy_444_zero_body;
use crate::layout::base_feature_legacy_zero_body as legacy_zero_body;
use crate::layout::base_feature_result_body_entry as result_body_entry;
use crate::layout::base_feature_result_body_prefix as result_body;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as class_296_legacy_scalar_54;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_70 as class_296_legacy_scalar_70;
use crate::layout::class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance;
use crate::layout::class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face;
use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face;
use crate::layout::class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric;
use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces;
use crate::layout::class_403_revolve_scope_frame as class_403_revolve;
use crate::layout::coil_compact_placement_identity_frame as coil_identity;
use crate::layout::coil_compact_placement_matrix_frame as coil_matrix;
use crate::layout::coil_compact_placement_owner_identity_frame as coil_owner_identity;
use crate::layout::coil_compact_scope_discriminators as coil_compact;
use crate::layout::coil_legacy_placement_identity_frame as coil_legacy_identity;
use crate::layout::coil_long_scope_fixed_prologue as coil_long;
use crate::layout::coil_modern_placement_matrix_frame as coil_modern_matrix;
use crate::layout::combine_compact_operation_prefix as combine_compact;
use crate::layout::combine_extended_reference_operation_prefix as combine_extended;
use crate::layout::combine_external_selector_prefix as combine_external;
use crate::layout::combine_standard_operation_prefix as combine_standard;
use crate::layout::compact_loft_operation_prefix as compact_loft;
use crate::layout::compact_shifted_extrude_extent_and_table_prefix as compact_extrude_extent;
use crate::layout::compact_shifted_extrude_mixed_extent_and_table_prefix as compact_extrude_mixed;
use crate::layout::compact_shifted_extrude_prologue as compact_extrude;
use crate::layout::component_insert_carrier_334_prefix as component_carrier_334;
use crate::layout::component_insert_identity_scope_compact as component_identity_scope;
use crate::layout::component_insert_identity_scope_shifted_prefix as component_identity_shifted;
use crate::layout::component_insert_matrix_scope_414_264_prefix as component_matrix_414;
use crate::layout::component_insert_relation_345_57 as component_insert_relation_345;
use crate::layout::component_insert_relation_child_393_58 as component_insert_relation_child_393;
use crate::layout::component_insert_scope_283_262_257 as component_scope_283_257;
use crate::layout::component_insert_scope_283_262_385 as component_scope_283_385;
use crate::layout::current_extrude_non_target_extent_pair as extrude_extent_pair;
use crate::layout::current_extrude_operation_fields as extrude_fields;
use crate::layout::current_extrude_shape_target_extent_prefix as extrude_target;
use crate::layout::derived_instance_relation_310_57 as derived_instance_relation_310;
use crate::layout::derived_instance_scope_279_261 as derived_instance_279_261;
use crate::layout::design_mirror_scope_class369_tail as mirror_369;
use crate::layout::design_mirror_scope_class391_tail as mirror_391;
use crate::layout::design_mirror_scope_class413_tail as mirror_413;
use crate::layout::design_mirror_scope_class440_tail as mirror_440;
use crate::layout::design_mirror_scope_class441_count_owner as mirror_441_count;
use crate::layout::design_mirror_scope_class441_tail as mirror_441;
use crate::layout::early_distance_extrude_absent_prefix as early_absent;
use crate::layout::early_distance_extrude_present_prefix as early_present;
use crate::layout::edge_flange_class286_two_sided_per_edge_fixed_operation as edge_flange_286_per_edge;
use crate::layout::edge_flange_class325_334_two_sided_per_edge_fixed_operation as edge_flange_325_per_edge;
use crate::layout::edge_flange_class364_per_edge_width_fixed_operation as edge_flange_364_width;
use crate::layout::edge_flange_fixed_operation_section as edge_flange;
use crate::layout::edge_flange_legacy_single_edge_fixed_operation as edge_flange_legacy;
use crate::layout::edge_flange_multi_edge_fixed_operation as edge_flange_multi;
use crate::layout::edge_flange_to_object_fixed_operation_section as flange_to_object;
use crate::layout::fixed_pipe_operation_prefix as fixed_pipe;
use crate::layout::hem_gap_length_fixed_operation_section as hem_gap;
use crate::layout::hem_rolled_fixed_operation_section as hem_rolled;
use crate::layout::hem_teardrop_fixed_operation_section as hem_teardrop;
use crate::layout::joint_origin_legacy_class_337_266_frame as joint_origin_class_337_266;
use crate::layout::legacy_class_338_two_sided_distance_extrude_frame as class_338_legacy;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as class_415;
use crate::layout::legacy_pipe_operation_prefix as legacy_pipe;
use crate::layout::marker_one_revolve_prologue as revolve;
use crate::layout::named_solid_primitive_prologue as solid_prologue;
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::shifted_cylinder_primitive_352_frame as shifted_cylinder_352;
use crate::layout::shifted_cylinder_primitive_502_frame as shifted_cylinder_502;
use crate::layout::shifted_extrude_offset_283_two_sided_tail as shifted_283;
use crate::layout::shifted_extrude_offset_profile_extent_lane as offset_lane;
use crate::layout::shifted_extrude_prologue as shifted_extrude;
use crate::layout::shifted_reference_aware_extrude_class_323_symmetric_prefix as shifted_reference_aware_323_symmetric;
use crate::layout::shifted_reference_aware_extrude_class_323_tail as shifted_reference_aware_323_tail;
use crate::layout::shifted_reference_aware_extrude_scope_prefix as shifted_reference_aware;
use crate::layout::thicken_class_347_scope_frame as thicken_347;
use crate::layout::thread_compact_construction_tail as thread_compact_tail;
use crate::layout::thread_compact_legacy_construction_tail as thread_compact_legacy_tail;
use crate::layout::thread_owner_marked_scope_prefix as thread_owner;
use crate::layout::thread_standard_construction_tail as thread_tail;
use crate::layout::thread_standard_legacy_construction_tail as thread_standard_legacy_tail;
use crate::layout::thread_standard_scope_prefix as thread_standard;
use crate::layout::work_axis_direct_carrier_class_297 as work_axis_297;
use crate::layout::work_axis_direct_carrier_class_335 as work_axis_335;
use crate::layout::work_plane_legacy_321_opaque_matrix_frame as work_plane_321_opaque;
use crate::layout::work_plane_legacy_325_matrix_frame as work_plane_325;
use crate::layout::work_plane_legacy_337_matrix_frame as work_plane_337;
use crate::layout::work_plane_legacy_class_256_matrix_frame as work_plane_class_256;
use crate::layout::work_plane_legacy_class_290_matrix_frame as work_plane_class_290;
use crate::layout::work_plane_legacy_class_322_332_matrix_frame as work_plane_class_322_332;
use crate::layout::work_plane_legacy_class_337_325_matrix_frame as work_plane_class_337_325;
use crate::layout::work_plane_legacy_class_400_matrix_frame as work_plane_legacy;
use crate::records::{
    ConstructionRecipe, DesignAssemblyAlignment, DesignAssemblyAxialOperandTarget,
    DesignAssemblyAxialSelectorIdentity, DesignAssemblyOperandFrame, DesignAssemblyOperandPath,
    DesignAssemblyOperandPathLink, DesignAssemblyOperandQualifier, DesignBaseFeatureConstruction,
    DesignBaseFlangeOperation, DesignBendPosition, DesignCircularPatternConstruction,
    DesignCoilExtent, DesignCoilPlacement, DesignCoilSection, DesignCoilSectionPlacement,
    DesignCoilSelection, DesignCombineBodySelection, DesignCombineExternalBodyIdentity,
    DesignCombineForm, DesignCombineOperation, DesignComponentInsertConstruction,
    DesignComponentOccurrence,  DesignCopyPasteBodiesOperation,
    DesignCopyPasteComponentOperation, DesignDerivedInstanceConstruction,
    DesignDirectFaceOperation, DesignDraftOperation, DesignEdgeFlangeHeightExtent,
    DesignEdgeFlangeOperation, DesignEdgeFlangeWidthParameterSource, DesignEdgeWidthMode,
    DesignEntityHeader, DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
    DesignExtrudePrologueReference, DesignExtrudeStart, DesignExtrudeTargetOrdinal,
    DesignFixedChamferDistance, DesignFixedChamferParameters, DesignFixedExtrudeDistance,
    DesignFixedExtrudeParameters, DesignFixedExtrudeScalar, DesignFixedFilletGroup,
    DesignFixedFilletParameters, DesignHemOperation, DesignHemParameterOwners,
    DesignHoleConstruction, DesignHoleFaceSelection, DesignMirrorConstruction,
    DesignMirrorScopeTolerance, DesignMoveOperation, DesignParameter, DesignParameterOwner,
    DesignParameterScope, DesignPathFeatureConstruction, DesignRecordHeader,
    DesignRectangularPatternConstruction, DesignRectangularPatternInstances,
    DesignRuledSurfaceCorner, DesignRuledSurfaceMethod, DesignRuledSurfaceOperation,
    DesignScaleOperation, DesignSheetMetalHeightDatum, DesignSolidPrimitive,
    DesignSurfaceExtendMethod, DesignSurfaceExtendOperation, DesignSurfaceOffsetOperation,
    DesignSurfaceOffsetSupport, DesignSurfaceStitchOperation, DesignThreadConstruction,
    DesignThreadForm, DesignWorkAxisConstruction, DesignWorkAxisSource,
    DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointRule,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

const EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8: f64 = 1.0e-8;
const EPS_SCOPES_SAME_TRANSFORM_BASIS_E10: f64 = 1.0e-10;
const EPS_SCOPES_EXACT_CIRCULAR_PATTERN_AXIS_E12: f64 = 1.0e-12;
const EPS_SCOPES_VALID_RIGHT_HANDED_COIL_TRANSFORM_E10: f64 = 1.0e-10;

mod assembly_carrier_paths;
pub(crate) mod extrude_sheet_metal;
pub(crate) mod legacy_class_397;
pub(crate) mod legacy_class_415;

use extrude_sheet_metal::{
    bind_hem_operation_from_parameters, exact_base_flange_operation, exact_edge_flange_operation,
    exact_extrude_prologue, exact_ruled_surface_operation, exact_surface_stitch_operation,
};

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub fn decode_parameter_scopes(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
    types: &[crate::records::SegmentType],
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::DesignParameterOwner],
    component_occurrences: &[DesignComponentOccurrence],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignParameterScope>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        let records = IndexedRecordOffsets::build(bytes);
        let stream_types = crate::design::decode::meta::stream_types_by_entity(types, &entry.name);
        let stream_scope_start = out.len();
        for header in parameter_scope_candidate_headers(bytes, &records) {
            let Some(mut scope) = parse_parameter_scope(bytes, &records, &header) else {
                continue;
            };
            scope.id = ids::native_design_parameter_scope_id(&entry.name, scope.byte_offset);
            bind_coil_extent_from_parameters(&mut scope, parameters, parameter_owners);
            bind_hem_operation_from_parameters(bytes, &mut scope, parameters, parameter_owners);
            if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch) {
                let start = usize::try_from(scope.byte_offset).ok();
                let end = usize::try_from(scope.paired_byte_offset).ok();
                let frame = start
                    .zip(end)
                    .and_then(|(start, end)| bytes.get(start..end));
                let mut matches = Vec::new();
                if let Some(frame) = frame {
                    // One pass over the frame: the first offset of every
                    // marked reference (a one byte, a u32 suffix, six zero
                    // bytes), then each eligible entity looks its suffix up.
                    let mut first_at: HashMap<u32, usize> = HashMap::new();
                    for at in memchr::memchr_iter(1, frame) {
                        if at + 11 <= frame.len() && frame[at + 5..at + 11] == [0; 6] {
                            if let Some(suffix) = View::u32_le_at(frame, at + 1) {
                                first_at.entry(suffix).or_insert(at);
                            }
                        }
                    }
                    for entity in entities {
                        if native_stream(&entity.id) != Some(stream.as_str())
                            || !entity.in_sketch_module()
                            || entity.entity_id.suffix() > u64::from(u32::MAX)
                        {
                            continue;
                        }
                        if let Some(at) = first_at.get(&(entity.entity_id.suffix() as u32)) {
                            matches.push((entity, at + 1));
                        }
                    }
                }
                if let [(entity, relative_offset)] = matches.as_slice() {
                    if let crate::records::DesignScopePayload::Sketch(slot)
                    | crate::records::DesignScopePayload::Esquisse(slot)
                    | crate::records::DesignScopePayload::Skizze(slot)
                    | crate::records::DesignScopePayload::Esboco(slot) = &mut scope.payload
                    {
                        *slot = Some(crate::records::DesignSketchEntityBinding {
                            entity_id: entity.entity_id.clone(),
                            entity_reference_offset: scope
                                .byte_offset
                                .saturating_add(*relative_offset as u64),
                        });
                    }
                }
            }
            if scope.kind() == crate::records::DesignFeatureKind::WorkPlane {
                if let Some(frame) = exact_work_plane_frame(bytes, &records, &scope) {
                    if let crate::records::DesignScopePayload::WorkPlane(slot) = &mut scope.payload
                    {
                        *slot = Some(crate::records::DesignWorkPlaneTransform {
                            work_plane_transform: frame.transform,
                            work_plane_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                crate::records::DesignWorkPlaneReference {
                                    work_plane_reference: record_index,
                                    work_plane_reference_offset: offset,
                                }
                            }),
                            work_plane_construction: None,
                        });
                    }
                }
            }
            if let Some(construction) = exact_work_axis_construction(bytes, &records, &scope) {
                if let crate::records::DesignScopePayload::WorkAxis(slot) = &mut scope.payload {
                    *slot = Some(construction);
                }
            }
            if scope.kind() == crate::records::DesignFeatureKind::JointOrigin {
                if let Some(frame) = exact_joint_origin_frame(bytes, &records, &scope) {
                    if let crate::records::DesignScopePayload::JointOrigin(slot) =
                        &mut scope.payload
                    {
                        *slot = Some(crate::records::DesignJointOriginTransform {
                            joint_origin_transform: frame.transform,
                            joint_origin_transform_offset: frame.transform_offset,
                            reference: frame.reference.map(|(record_index, offset)| {
                                crate::records::DesignJointOriginReference {
                                    joint_origin_reference: record_index,
                                    joint_origin_reference_offset: offset,
                                }
                            }),
                        });
                    }
                }
            }
            {
                let construction =
                    exact_work_point_construction(bytes, &records, &scope, &stream_types);
                if let crate::records::DesignScopePayload::WorkPoint(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            {
                let construction = exact_hole_construction(bytes, &records, &scope, &stream_types);
                if let crate::records::DesignScopePayload::Hole(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            if let Some(placement) = exact_coil_placement(bytes, &records, &scope, recipes) {
                if let crate::records::DesignScopePayload::SpirePrimitive(slot)
                | crate::records::DesignScopePayload::CoilPrimitive(slot) = &mut scope.payload
                {
                    slot.get_or_insert_with(Default::default).coil_placement = Some(placement);
                }
            }
            if let Some(construction) = exact_solid_primitive(bytes, &records, &scope, parameter_owners) {
                scope.payload = construction.into();
            }
            {
                let construction = exact_direct_face_operation(bytes, &records, &scope);
                match (&mut scope.payload, construction) {
(crate::records::DesignScopePayload::OffsetFaces(slot) | crate::records::DesignScopePayload::DecalerLesFaces(slot), Some(crate::records::DesignDirectFaceOperation::OffsetFaces(value))) => *slot = Some(value),
(crate::records::DesignScopePayload::Shell(slot) | crate::records::DesignScopePayload::Schale(slot), Some(crate::records::DesignDirectFaceOperation::Shell(value))) => *slot = Some(value),
(crate::records::DesignScopePayload::Thicken(slot), Some(crate::records::DesignDirectFaceOperation::Thicken(value))) => *slot = Some(value),
_ => {},
}
            }
            {
                let construction = exact_move_operation(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::Move(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            {
                let construction = exact_scale_operation(bytes, &records, &scope, &stream_types);
                if let crate::records::DesignScopePayload::Scale(slot)
                | crate::records::DesignScopePayload::Massstab(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_extend_operation(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::SurfaceExtend(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_surface_offset_operation(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::SurfaceOffset(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            if let Some(parameters) = exact_fixed_extrude_parameters(
                bytes,
                &records,
                &scope,
                parameters,
                parameter_owners,
            ) {
                {
                    let value = Some(parameters);
                    if let crate::records::DesignScopePayload::Extrude(slot)
                    | crate::records::DesignScopePayload::Extrusion(slot)
                    | crate::records::DesignScopePayload::Extrusao(slot) = &mut scope.payload
                    {
                        slot.get_or_insert_with(Default::default)
                            .fixed_extrude_parameters = value;
                    }
                }
            }
            {
                let construction = exact_fixed_fillet_parameters(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::Fillet(slot)
                | crate::records::DesignScopePayload::Conge(slot)
                | crate::records::DesignScopePayload::Abrundung(slot)
                | crate::records::DesignScopePayload::Arredondamento(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_fixed_chamfer_parameters(bytes, &records, &scope, parameter_owners);
                if let crate::records::DesignScopePayload::Chamfer(slot)
                | crate::records::DesignScopePayload::Chanfrein(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            if let Some(construction) =
                exact_path_feature_construction(bytes, &records, &scope, parameter_owners)
            {
                scope.payload = construction.into();
            }
            {
                let construction = exact_combine_operation(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::Combine(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            {
                let construction = exact_thread_construction(bytes, &scope);
                if let crate::records::DesignScopePayload::Thread(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_draft_operation_with_owners(bytes, &records, &scope, parameter_owners);
                if let crate::records::DesignScopePayload::Draft(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            {
                let construction = exact_circular_pattern_construction_with_owners(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                );
                if let crate::records::DesignScopePayload::CPattern(slot)
                | crate::records::DesignScopePayload::CircularPattern(slot)
                | crate::records::DesignScopePayload::ReseauC(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_rectangular_pattern_construction(
                    bytes,
                    &records,
                    &scope,
                    parameter_owners,
                );
                if let crate::records::DesignScopePayload::RPattern(slot)
                | crate::records::DesignScopePayload::RectangularPattern(slot) =
                    &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction =
                    exact_assembly_alignment(bytes, &records, &scope, parameter_owners);
                if let crate::records::DesignScopePayload::Assemble(slot)
                | crate::records::DesignScopePayload::AsBuilt(slot) = &mut scope.payload
                {
                    *slot = construction;
                }
            }
            let legacy_form = scope.assembly_alignment().and_then(|alignment| {
                let crate::records::DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, limits } = alignment.form.as_ref()? else { return None; };
                let carriers = exact_legacy_as_built_421_operands(bytes, &records, &scope, &stream_types, recipes, solved_frame)?;
                Some(crate::records::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                    carriers,
                    solved_frame: solved_frame.clone(),
                    limits: limits.clone(),
                    frames_field_present: true,
                })
            });
            if let (Some(alignment), Some(form)) = (scope.assembly_alignment_mut(), legacy_form) {
                alignment.form = Some(form);
            }
            {
                let construction = exact_component_insert_construction(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::ComponentInsert(slot) =
                    &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_derived_instance_construction(
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                );
                if let crate::records::DesignScopePayload::DerivedInstance(slot) =
                    &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_copy_paste_component_operation(
                    bytes,
                    &records,
                    &scope,
                    component_occurrences,
                );
                if let crate::records::DesignScopePayload::CopyPaste(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            bind_component_pattern_occurrences(&mut scope, component_occurrences);
            {
                let construction = exact_copy_paste_bodies_operation(bytes, &records, &scope);
                if let crate::records::DesignScopePayload::CopyPasteBodies(slot) =
                    &mut scope.payload
                {
                    *slot = construction;
                }
            }
            {
                let construction = exact_base_feature_construction(bytes, &scope);
                if let crate::records::DesignScopePayload::BaseFeature(slot) = &mut scope.payload {
                    *slot = construction;
                }
            }
            out.push(scope);
        }
        bind_joint_origin_frames_from_assemblies(bytes, &mut out[stream_scope_start..]);
        bind_axial_assembly_operand_targets(bytes, &records, &mut out[stream_scope_start..]);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

/// Admit one envelope for every logical scope identity.
///
/// Some Design streams retain more than one complete envelope for one record
/// index. A history-bound envelope is authoritative when exactly one
/// candidate resolves to a unique ASM state transition; an unresolved group
/// remains an error so a duplicate cannot be selected by byte order.
pub(crate) fn admit_history_bound_scope_variants(
    scopes: &mut Vec<DesignParameterScope>,
    histories: &[crate::history_records::AsmHistory],
) -> Result<(), CodecError> {
    let mut groups = HashMap::<(String, u32), Vec<usize>>::new();
    for (index, scope) in scopes.iter().enumerate() {
        let stream = native_stream(&scope.id)
            .unwrap_or(ids::DEFAULT_STREAM)
            .to_owned();
        groups
            .entry((stream, scope.record_index))
            .or_default()
            .push(index);
    }

    let mut admitted =
        cadmpeg_core::decode::alloc_filled(scopes.len(), true, "f3d scope admission")?;
    for indices in groups.values().filter(|indices| indices.len() > 1) {
        let history_bound = indices
            .iter()
            .copied()
            .filter(|index| {
                let scope = &scopes[*index];
                let Some(state_id) = scope.history_state_id else {
                    return false;
                };
                let Some(previous_state_id) =
                    crate::history::effective_scope_previous_history_state_id(scope, histories)
                else {
                    return false;
                };
                crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
                    .is_some()
            })
            .collect::<Vec<_>>();
        let equivalent_payload = indices.first().is_some_and(|first| {
            indices
                .iter()
                .skip(1)
                .all(|index| equivalent_scope_variant_payload(&scopes[*first], &scopes[*index]))
        });
        let keep = match history_bound.as_slice() {
            [keep] => *keep,
            [] if equivalent_payload => *indices
                .iter()
                .max_by_key(|index| scopes[**index].byte_offset)
                .expect("equivalent duplicate scope group is non-empty"),
            _ => {
                return Err(CodecError::Malformed(
                    "Design scope record identity has unresolved duplicate envelopes".into(),
                ));
            }
        };
        for index in indices {
            admitted[*index] = *index == keep;
        }
    }

    let retained = std::mem::take(scopes)
        .into_iter()
        .enumerate()
        .filter_map(|(index, scope)| admitted[index].then_some(scope))
        .collect();
    *scopes = retained;
    Ok(())
}

/// Compare two same-index scope envelopes after removing source-location and
/// dynamic-class fields. An equivalent envelope is one serialization of the
/// same logical scope; the later envelope supersedes the earlier one when no
/// decoded ASM state pair can select a revision.
fn equivalent_scope_variant_payload(
    left: &DesignParameterScope,
    right: &DesignParameterScope,
) -> bool {
    let (Ok(mut left), Ok(mut right)) = (serde_json::to_value(left), serde_json::to_value(right))
    else {
        return false;
    };
    strip_scope_variant_provenance(&mut left, true);
    strip_scope_variant_provenance(&mut right, true);
    left == right
}

fn strip_scope_variant_provenance(value: &mut serde_json::Value, top_level: bool) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                strip_scope_variant_provenance(item, false);
            }
        }
        serde_json::Value::Object(fields) => {
            fields.retain(|key, _| {
                if key.ends_with("_offset") || key.ends_with("_offsets") {
                    return false;
                }
                if top_level
                    && matches!(
                        key.as_str(),
                        "id" | "class_tag"
                            | "frame_length"
                            | "history_state_id"
                            | "previous_history_state_id"
                            | "paired_class_tag"
                    )
                {
                    return false;
                }
                true
            });
            for field in fields.values_mut() {
                strip_scope_variant_provenance(field, false);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

pub(crate) fn exact_thread_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignThreadConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    if scope.kind() != crate::records::DesignFeatureKind::Thread
        || scope.reference_members.len() < 2
        || !scope.reference_members.len().is_multiple_of(2)
    {
        return None;
    }
    let (prefix_form, designation_delta) = exact_thread_prefix(bytes.get(start..)?)?;
    let designation_at = start.checked_add(designation_delta)?;
    let face_group_record_indices = match prefix_form {
        ThreadPrefix::Standard => vec![*scope.reference_members.values().next()?],
        ThreadPrefix::Compact => scope.reference_members.values().step_by(2).copied().collect(),
    };
    let construction = parse_thread_payload(
        bytes,
        designation_at,
        prefix_form,
        face_group_record_indices,
    )?;
    let class_pair_is_valid = match construction.form {
        DesignThreadForm::StandardLegacy => {
            scope.class_tag == "334" && scope.paired_class_tag == "262"
        }
        DesignThreadForm::CompactLegacy => {
            scope.class_tag == "414" && scope.paired_class_tag == "263"
        }
        DesignThreadForm::Standard | DesignThreadForm::Compact(_) => true,
    };
    if !class_pair_is_valid {
        return None;
    }
    Some(construction)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadPrefix {
    Standard,
    Compact,
}

fn exact_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    let direct = if bytes.get(thread_standard::ZERO_RUN_10..thread_standard::FIXED_SCALAR)?
        == [0; 10]
        && View::f64_le_at(bytes, thread_standard::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(
            bytes,
            thread_standard::STANDARD_MARKER,
            thread_standard::STANDARD_PREFIX_TAIL,
        )
        .map(|form| (form, thread_standard::LEN))
    } else {
        None
    };
    let owner_marked = if bytes.get(thread_owner::ZERO_RUN_9..thread_owner::OWNER_MARKER)? == [0; 9]
        && View::u32_le_at(bytes, thread_owner::OWNER_MARKER)? == 1
        && bytes.get(thread_owner::SEPARATOR) == Some(&0)
        && View::f64_le_at(bytes, thread_owner::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(bytes, thread_owner::FORM_MARKER, thread_owner::FORM_TOKEN)
            .map(|form| (form, thread_owner::LEN))
    } else {
        None
    };
    match (direct, owner_marked) {
        (Some(prefix), None) | (None, Some(prefix)) => Some(prefix),
        _ => None,
    }
}

fn thread_form(bytes: &[u8], marker_at: usize, token_at: usize) -> Option<ThreadPrefix> {
    match (
        bytes.get(marker_at..marker_at + 5)?,
        bytes.get(token_at..token_at + 4)?,
    ) {
        ([1, 2, 0, 0, 0], [0x36, 0, 0x67, 0]) => Some(ThreadPrefix::Standard),
        ([0, 2, 0, 0, 0], [0x36, 0, 0x48, 0]) => Some(ThreadPrefix::Compact),
        _ => None,
    }
}

pub(crate) fn parse_thread_payload(
    bytes: &[u8],
    designation_at: usize,
    expected_form: ThreadPrefix,
    face_group_record_indices: Vec<u32>,
) -> Option<DesignThreadConstruction> {
    let (designation, after_designation) = lp_utf16_bounded(bytes, designation_at, 1..=128)?;
    let (nominal_size_text, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
    let (profile, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
    let (pitch_marker, trailer_kind) =
        match (expected_form, bytes.get(after_profile..after_profile + 5)?) {
            (ThreadPrefix::Standard, [0, 1, 0, 0, 0]) => {
                (1, ThreadTrailerKind::Standard)
            }
            (ThreadPrefix::Standard, [1, 1, 0, 0, 0]) => (
                0,
                ThreadTrailerKind::StandardLegacy,
            ),
            (ThreadPrefix::Compact, [1, 2, 0, 0, 0]) => {
                (0, ThreadTrailerKind::Compact)
            }
            (ThreadPrefix::Compact, [1, 1, 0, 0, 0]) => (
                0,
                ThreadTrailerKind::CompactLegacy,
            ),
            _ => return None,
        };
    let nominal_size = crate::records::DesignThreadNominalSize::try_from(nominal_size_text).ok()?;
    let major_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MAJOR_DIAMETER)?;
    let minor_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MINOR_DIAMETER)?;
    let pitch = (bytes.get(after_profile + thread_tail::PITCH_MARKER) == Some(&pitch_marker))
        .then(|| View::f64_le_at(bytes, after_profile + thread_tail::PITCH))??;
    let pitch_diameter = View::f64_le_at(bytes, after_profile + thread_tail::PITCH_DIAMETER)?;
    let trailer_at = match trailer_kind {
        ThreadTrailerKind::Standard => thread_tail::STANDARD_TRAILER,
        ThreadTrailerKind::Compact => thread_compact_tail::COMPACT_TRAILER,
        ThreadTrailerKind::StandardLegacy => thread_standard_legacy_tail::LEGACY_TRAILER,
        ThreadTrailerKind::CompactLegacy => thread_compact_legacy_tail::LEGACY_TRAILER,
    };
    let trailer_offset = after_profile.checked_add(trailer_at)?;
    let form = match trailer_kind {
        ThreadTrailerKind::Standard if bytes.get(trailer_offset..trailer_offset + 2)? == [0, 1] => {
            DesignThreadForm::Standard
        }
        ThreadTrailerKind::StandardLegacy
            if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
        { DesignThreadForm::StandardLegacy }
        ThreadTrailerKind::CompactLegacy
            if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
        { DesignThreadForm::CompactLegacy }
        ThreadTrailerKind::Compact
            if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
        {
            DesignThreadForm::Compact(None)
        }
        ThreadTrailerKind::Compact
            if bytes.get(trailer_offset) == Some(&1)
                && bytes.get(trailer_offset + 5..trailer_offset + 11)? == [0; 6] =>
        {
            let reference_offset = trailer_offset.checked_add(1)?;
            let record_index = std::num::NonZeroU32::new(View::u32_le_at(bytes, reference_offset)?)?;
            DesignThreadForm::Compact(Some(crate::records::Located {
                value: record_index,
                offset: u64::try_from(reference_offset).ok()?,
            }))
        }
        _ => return None,
    };
    if !([
        major_diameter,
        minor_diameter,
        pitch,
        pitch_diameter,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value > 0.0)
        && minor_diameter < pitch_diameter
        && pitch_diameter < major_diameter)
    {
        return None;
    }
    Some(DesignThreadConstruction {
        form,
        designation_offset: u64::try_from(designation_at).ok()?,
        designation,
        nominal_size,
        profile,
        major_diameter,
        minor_diameter,
        pitch,
        pitch_diameter,
        face_group_record_indices,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadTrailerKind {
    Standard,
    Compact,
    StandardLegacy,
    CompactLegacy,
}

pub(crate) fn bind_joint_origin_frames_from_assemblies(
    bytes: &[u8],
    scopes: &mut [DesignParameterScope],
) {
    let mut candidates = Vec::new();
    let mut envelopes = Vec::new();
    for scope in scopes.iter() {
        if scope.kind() != crate::records::DesignFeatureKind::Assemble {
            continue;
        }
        if let Some(frames) = scope
            .assembly_alignment()
            .and_then(|alignment| alignment.operand_frames())
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
            envelopes.push((scope.record_index, joint_origin, frame.transform));
            candidates.push((
                joint_origin,
                frame.transform,
                frame.transform_offset,
                frame.reference,
            ));
        }
    }
    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::DesignFeatureKind::JointOrigin
            && scope.joint_origin_frame().is_none()
    }) {
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
        {
            let construction = Some(crate::records::DesignJointOriginTransform {
                joint_origin_transform: *transform,
                joint_origin_transform_offset: *transform_offset,
                reference: reference.map(|(record_index, offset)| {
                    crate::records::DesignJointOriginReference {
                        joint_origin_reference: record_index,
                        joint_origin_reference_offset: offset,
                    }
                }),
            });
            if let crate::records::DesignScopePayload::JointOrigin(slot) = &mut scope.payload {
                *slot = construction;
            }
        }
    }
    let resolved_origins = scopes
        .iter()
        .filter(|scope| scope.kind() == crate::records::DesignFeatureKind::JointOrigin)
        .filter_map(|scope| Some((scope.record_index, scope.joint_origin_transform()?)))
        .collect::<HashMap<_, _>>();
    for (assembly_record_index, joint_origin_record_index, transform) in envelopes {
        if resolved_origins.get(&joint_origin_record_index) != Some(&transform) {
            continue;
        }
        let mut assemblies = scopes.iter_mut().filter(|scope| {
            scope.kind() == crate::records::DesignFeatureKind::Assemble
                && scope.record_index == assembly_record_index
        });
        let Some(assembly) = assemblies.next() else {
            continue;
        };
        if assemblies.next().is_some() {
            continue;
        }
        if let Some(alignment) = assembly.assembly_alignment_mut() {
            alignment.form = Some(crate::records::DesignAssemblyAlignmentForm::DatumEnvelope { joint_origin_scope_record_index: joint_origin_record_index });
        }
    }
}

/// Bind the pathless axial assembly selectors after every scope in the Design
/// stream has decoded its own construction.
pub(crate) fn bind_axial_assembly_operand_targets(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scopes: &mut [DesignParameterScope],
) {
    let bindings = scopes
        .iter()
        .enumerate()
        .filter_map(|(ordinal, scope)| {
            if !matches!(scope.frame_length, 705 | 772) {
                return None;
            }
            let alignment = scope.assembly_alignment()?;
            let crate::records::DesignAssemblyAlignmentForm::Frames { frames } = alignment.form.as_ref()? else { return None; };
            let first =
                exact_assembly_axial_operand_target(bytes, records, scope, &frames[0], scopes)?;
            let second =
                exact_assembly_axial_operand_target(bytes, records, scope, &frames[1], scopes)?;
            Some((
                ordinal,
                crate::records::DesignAssemblyAlignmentForm::qualified(frames.clone(), [first, second]
                    .map(|target| DesignAssemblyOperandQualifier::AxialTarget { target })),
            ))
        })
        .collect::<Vec<_>>();

    for (ordinal, form) in bindings {
        if let Some(alignment) = scopes[ordinal].assembly_alignment_mut() {
            alignment.form = Some(form);
        }
    }
}

struct AxialComponentOperand {
    construction_record_index: u32,
    construction_class_tag: String,
    construction_byte_offset: u64,
    construction_transform_offset: u64,
    axis_record_index_offsets: [u64; 2],
    construction_paired_class_tag: String,
    construction_paired_byte_offset: u64,
    selectors: Box<[DesignAssemblyAxialSelectorIdentity; 2]>,
}

struct ExactIndexedRecordPair {
    record_index: u32,
    class_tag: String,
    byte_offset: usize,
    paired_class_tag: String,
    paired_byte_offset: usize,
}

fn exact_assembly_axial_operand_target(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    assembly: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    scopes: &[DesignParameterScope],
) -> Option<DesignAssemblyAxialOperandTarget> {
    let component = exact_assembly_axial_component_operand(bytes, records, assembly, frame)
        .and_then(|component| {
            let role = &component.selectors[0].occurrence_role;
            let mut matches = scopes.iter().filter(|scope| {
                scope.kind() == crate::records::DesignFeatureKind::ComponentInsert
                    && scope
                        .component_insert_construction()
                        .is_some_and(|construction| {
                            construction.neutron_role.eq_ignore_ascii_case(role)
                        })
            });
            let component_insert = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            Some(
                DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                    component_insert_scope_record_index: component_insert.record_index,
                    construction_record_index: component.construction_record_index,
                    construction_class_tag: component.construction_class_tag,
                    construction_byte_offset: component.construction_byte_offset,
                    construction_transform_offset: component.construction_transform_offset,
                    axis_record_index_offsets: component.axis_record_index_offsets,
                    construction_paired_class_tag: component.construction_paired_class_tag,
                    construction_paired_byte_offset: component.construction_paired_byte_offset,
                    selectors: component.selectors,
                },
            )
        });
    let mut origins = scopes.iter().filter(|scope| {
        scope.kind() == crate::records::DesignFeatureKind::JointOrigin
            && scope.record_index == frame.reference_record_index
            && scope.joint_origin_transform() == Some(frame.transform)
    });
    let root = match (origins.next(), origins.next()) {
        (Some(origin), None) => Some(DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
            scope_record_index: origin.record_index,
        }),
        _ => None,
    };
    match (component, root) {
        (Some(target), None) | (None, Some(target)) => Some(target),
        _ => None,
    }
}

fn exact_assembly_axial_component_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
) -> Option<AxialComponentOperand> {
    if !matches!(scope.frame_length, 705 | 772)
        || scope
            .reference_members
            .values()
            .filter(|record_index| **record_index == frame.reference_record_index)
            .count()
            != 1
    {
        return None;
    }
    let search_start = usize::try_from(scope.paired_byte_offset).ok()?;
    let mut candidates = records
        .offsets(frame.reference_record_index)
        .iter()
        .copied()
        .filter(|start| *start >= search_start)
        .filter_map(|start| {
            exact_assembly_axial_component_operand_at(bytes, records, scope, frame, start)
        });
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(candidate)
}

fn exact_assembly_axial_component_operand_at(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    start: usize,
) -> Option<AxialComponentOperand> {
    let construction_class_tag =
        exact_indexed_header_at(bytes, start, frame.reference_record_index)?;
    let paired_at = start.checked_add(axial_carrier::PAIRED_INDEXED_HEADER)?;
    let construction_paired_class_tag =
        exact_indexed_header_at(bytes, paired_at, frame.reference_record_index)?;
    let construction_transform_at = start.checked_add(axial_carrier::OPERAND_TRANSFORM)?;
    if rigid_transform_at(bytes, construction_transform_at)? != frame.transform {
        return None;
    }
    let (first_axis_record_index, first_axis_record_index_offset) =
        exact_same_segment_record_reference(
            bytes,
            start.checked_add(axial_carrier::FIRST_AXIS_RECORD_REFERENCE)?,
        )?;
    let (second_axis_record_index, second_axis_record_index_offset) =
        exact_same_segment_record_reference(
            bytes,
            start.checked_add(axial_carrier::SECOND_AXIS_RECORD_REFERENCE)?,
        )?;
    if first_axis_record_index == second_axis_record_index {
        return None;
    }
    let first_selector_record_index = first_axis_record_index.checked_add(3)?;
    let second_selector_record_index = second_axis_record_index.checked_add(3)?;
    for pair in [
        [first_axis_record_index, first_selector_record_index],
        [second_axis_record_index, second_selector_record_index],
    ] {
        if scope.reference_members.values().zip(scope.reference_members.values().skip(1))
            .filter(|(first, second)| [**first, **second] == pair)
            .count()
            != 1
            || pair.iter().any(|record_index| {
                scope
                    .reference_members
                    .values()
                    .filter(|member| *member == record_index)
                    .count()
                    != 1
            })
        {
            return None;
        }
    }
    let search_start = usize::try_from(scope.paired_byte_offset).ok()?;
    let first_axis = exact_paired_indexed_record_between(
        bytes,
        records,
        first_axis_record_index,
        search_start,
        start,
    )?;
    let second_axis = exact_paired_indexed_record_between(
        bytes,
        records,
        second_axis_record_index,
        search_start,
        start,
    )?;
    if first_axis.byte_offset >= second_axis.byte_offset {
        return None;
    }
    let second_axis_at = second_axis.byte_offset;
    let first = exact_assembly_axial_selector(bytes, records, first_axis, second_axis_at)?;
    let second = exact_assembly_axial_selector(bytes, records, second_axis, start)?;
    if !first.selects_same_object(&second)
        || !first
            .occurrence_role
            .eq_ignore_ascii_case(&second.occurrence_role)
    {
        return None;
    }
    Some(AxialComponentOperand {
        construction_record_index: frame.reference_record_index,
        construction_class_tag,
        construction_byte_offset: u64::try_from(start).ok()?,
        construction_transform_offset: u64::try_from(construction_transform_at).ok()?,
        axis_record_index_offsets: [
            first_axis_record_index_offset,
            second_axis_record_index_offset,
        ],
        construction_paired_class_tag,
        construction_paired_byte_offset: u64::try_from(paired_at).ok()?,
        selectors: Box::new([first, second]),
    })
}

fn exact_assembly_axial_selector(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    axis: ExactIndexedRecordPair,
    limit: usize,
) -> Option<DesignAssemblyAxialSelectorIdentity> {
    let axis_record_index = axis.record_index;
    let selector_record_index = axis_record_index.checked_add(3)?;
    let selector_offsets = records
        .offsets(selector_record_index)
        .iter()
        .copied()
        .filter(|offset| *offset > axis.paired_byte_offset && *offset < limit)
        .collect::<Vec<_>>();
    let [selector_at, selector_paired_at] = selector_offsets.as_slice() else {
        return None;
    };
    let selector_at = *selector_at;
    let selector_paired_at = *selector_paired_at;
    let selector_class_tag = exact_indexed_header_at(bytes, selector_at, selector_record_index)?;
    let selector_paired_class_tag =
        exact_indexed_header_at(bytes, selector_paired_at, selector_record_index)?;
    if bytes.get(
        selector_at.checked_add(axial_selector::ZERO_RUN_11)?
            ..selector_at.checked_add(axial_selector::NESTED_RECORD_REFERENCE)?,
    )? != [0; 11]
    {
        return None;
    }
    let mut cursor = selector_at.checked_add(axial_selector::NESTED_RECORD_REFERENCE)?;
    let nested_record_index_offset = cursor.checked_add(1)?;
    let (nested_record_index, _) = exact_same_segment_record_reference(bytes, cursor)?;
    cursor = cursor.checked_add(11)?;
    if nested_record_index != selector_record_index.checked_add(3)?
        || View::u32_le_at(bytes, cursor)? != 1
    {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let selector_asset_at = cursor;
    let (selector_asset_id, after_selector_asset_id) =
        lp_utf16_bounded(bytes, selector_asset_at, 36..=36)?;
    let selector_context_at = after_selector_asset_id;
    let (selector_context_id, after_selector_context_id) =
        lp_utf16_bounded(bytes, selector_context_at, 36..=36)?;
    if !is_guid_relaxed(&selector_asset_id)
        || !is_guid_relaxed(&selector_context_id)
        || View::u32_le_at(bytes, after_selector_context_id)? != 2
        || View::u32_le_at(bytes, after_selector_context_id.checked_add(4)?)? != 0
        || View::u32_le_at(bytes, after_selector_context_id.checked_add(8)?)? != 1
    {
        return None;
    }
    cursor = after_selector_context_id.checked_add(12)?;
    let occurrence_reference_offset = cursor.checked_add(1)?;
    let occurrence = take_reference(bytes, &mut cursor)?;
    let occurrence_reference = occurrence.target?;
    if occurrence_reference == 0
        || occurrence.segment.is_some()
        || occurrence.link_name.is_some()
        || View::u32_le_at(bytes, cursor)? != 1
    {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let external = take_external_reference_identity(bytes, &mut cursor)?;
    if !external.asset_id.eq_ignore_ascii_case(&selector_asset_id) || cursor > selector_paired_at {
        return None;
    }

    let role_record_index = selector_record_index.checked_add(5)?;
    let role_offsets = records
        .offsets(role_record_index)
        .iter()
        .copied()
        .filter(|offset| *offset > selector_paired_at && *offset < limit)
        .collect::<Vec<_>>();
    let [role_at] = role_offsets.as_slice() else {
        return None;
    };
    let role_at = *role_at;
    let role_class_tag = exact_indexed_header_at(bytes, role_at, role_record_index)?;
    if bytes.get(
        role_at.checked_add(axial_role::ZERO_RUN_10)?
            ..role_at.checked_add(axial_role::CONSTANT_ONE)?,
    )? != [0; 10]
        || View::u32_le_at(bytes, role_at.checked_add(axial_role::CONSTANT_ONE)?)? != 1
    {
        return None;
    }
    let occurrence_role_at = role_at.checked_add(axial_role::ROLE_CODE_UNIT_COUNT)?;
    let (occurrence_role, after_occurrence_role) =
        lp_utf16_bounded(bytes, occurrence_role_at, 36..=36)?;
    if !is_guid_relaxed(&occurrence_role) || after_occurrence_role > limit {
        return None;
    }

    Some(DesignAssemblyAxialSelectorIdentity {
        axis_record_index,
        axis_class_tag: axis.class_tag,
        axis_byte_offset: u64::try_from(axis.byte_offset).ok()?,
        axis_paired_class_tag: axis.paired_class_tag,
        axis_paired_byte_offset: u64::try_from(axis.paired_byte_offset).ok()?,
        selector_record_index,
        selector_class_tag,
        selector_byte_offset: u64::try_from(selector_at).ok()?,
        selector_paired_class_tag,
        selector_paired_byte_offset: u64::try_from(selector_paired_at).ok()?,
        nested_record_index,
        nested_record_index_offset: u64::try_from(nested_record_index_offset).ok()?,
        selector_asset_id,
        selector_asset_id_offset: u64::try_from(selector_asset_at.checked_add(4)?).ok()?,
        selector_context_id,
        selector_context_id_offset: u64::try_from(selector_context_at.checked_add(4)?).ok()?,
        occurrence_reference,
        occurrence_reference_offset: u64::try_from(occurrence_reference_offset).ok()?,
        external_object_reference: external.target,
        external_object_reference_offset: external.target_offset,
        external_segment: external.segment,
        external_segment_offset: external.segment_offset,
        external_asset_id: external.asset_id,
        external_asset_id_offset: external.asset_id_offset,
        external_link_name: external.link_name,
        external_link_name_offset: external.link_name_offset,
        external_version: external.version,
        role_record_index,
        role_class_tag,
        role_byte_offset: u64::try_from(role_at).ok()?,
        occurrence_role,
        occurrence_role_offset: u64::try_from(occurrence_role_at.checked_add(4)?).ok()?,
    })
}

fn exact_paired_indexed_record_between(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    start: usize,
    end: usize,
) -> Option<ExactIndexedRecordPair> {
    let offsets = records
        .offsets(record_index)
        .iter()
        .copied()
        .filter(|offset| *offset >= start && *offset < end)
        .collect::<Vec<_>>();
    let [primary_at, paired_at] = offsets.as_slice() else {
        return None;
    };
    let class_tag = exact_indexed_header_at(bytes, *primary_at, record_index)?;
    let paired_class_tag = exact_indexed_header_at(bytes, *paired_at, record_index)?;
    Some(ExactIndexedRecordPair {
        record_index,
        class_tag,
        byte_offset: *primary_at,
        paired_class_tag,
        paired_byte_offset: *paired_at,
    })
}

pub(crate) fn exact_indexed_header_at(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<String> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 3..=3, u8::is_ascii_digit)?;
    (View::u32_le_at(bytes, after_tag)? == record_index).then_some(class_tag)
}

fn exact_same_segment_record_reference(bytes: &[u8], at: usize) -> Option<(u32, u64)> {
    let mut cursor = at;
    let reference = take_reference(bytes, &mut cursor)?;
    let target = u32::try_from(reference.target?).ok()?;
    (cursor == at.checked_add(11)? && reference.segment.is_none() && reference.link_name.is_none())
        .then_some((target, u64::try_from(at.checked_add(1)?).ok()?))
}

fn exact_single_joint_origin_frame(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(u32, ScopePlacementFrame)> {
    if scope.kind() != crate::records::DesignFeatureKind::Assemble
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
        || View::u32_le_at(bytes, start + 175)? != 1
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
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::SurfaceOffset) {
        return None;
    }
    let distance_record_index = scope.reference_members.values().next()?;
    let support_reference_count = scope.reference_members.len() - 1;
    if support_reference_count == 0 {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, *distance_record_index)?;
    if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 0 {
        return None;
    }

    let mut group_record_indices = Vec::new();
    let mut covered_references = HashSet::new();
    for (scope_reference_ordinal, record_index) in
        scope.reference_members.values().copied().enumerate().skip(1)
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
        for member in group.members.iter().map(|member| &member.value) {
            if *member == *distance_record_index
                || !scope.reference_members.values().skip(1).any(|value| value == member)
                || !covered_references.insert(*member)
            {
                return None;
            }
        }
        group_record_indices.push(group.record_index);
    }
    if group_record_indices.is_empty() || covered_references.len() != support_reference_count {
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
    if design_feature_family(&scope.kind()) != Some(family) {
        return None;
    }
    let mut references = scope.reference_members.values();
    let distance_record_index = references.next()?;
    let boundary_record_index = references.next()?;
    let edge_record_indices = references;
    if edge_record_indices.len() == 0 {
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
                    && View::u32_le_at(bytes, *start + 59).is_some_and(|value| value != 0)
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
                || View::u32_le_at(bytes, start + 21)?
                    != u32::try_from(edge_record_indices.len()).ok()?
                || edge_record_indices
                    .clone()
                    .enumerate()
                    .any(|(ordinal, record_index)| {
                        marked_record_reference(bytes, start + 25 + ordinal * 11)
                            != Some(*record_index)
                    })
                || bytes.get(tail..tail + 2)? != [0; 2]
                || bytes.get(tail + 11..tail + 21)? != [0; 10]
                || View::u32_le_at(bytes, tail + 21)? != boundary_kind
                || bytes.get(tail + 25..tail + 35)? != [0; 10]
                || View::u32_le_at(bytes, tail + 35)? != 210
                || View::u32_le_at(bytes, tail + 47)? != 210
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
            let mode = View::u32_le_at(bytes, tail + 2)?;
            let boundary_reference_record_index = marked_record_reference(bytes, tail + 6)?;
            let tolerance = View::f64_le_at(bytes, tail + 39)?;
            (tolerance.is_finite() && tolerance > 0.0).then_some(ExactSurfaceBoundaryOperation {
                distance: scalar.value,
                distance_offset: scalar.value_offset,
                distance_record_index: *distance_record_index,
                mode,
                mode_offset: u64::try_from(tail + 2).ok()?,
                boundary_record_index: *boundary_record_index,
                boundary_reference_record_index,
                boundary_reference_offset: u64::try_from(tail + 6).ok()?,
                edge_record_indices: edge_record_indices.clone().copied().collect(),
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
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Assemble) {
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
    let as_built_421 = crate::design::assembly::legacy_as_built_421_generation(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )
    .is_some();
    let legacy_class_383 = crate::design::assembly::legacy_class_383_258_scope(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    );
    let legacy_class_388 = matches!(
        crate::design::assembly::operand_frame_variant(
            scope.frame_length,
            &scope.class_tag,
            &scope.paired_class_tag,
        ),
        Some(crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388)
    );
    if legacy_class_388 {
        exact_legacy_class_388_scope(bytes, scope)?;
    }
    use crate::records::DesignAssemblyAlignmentForm;
    if as_built_421 {
        let exact = exact_legacy_as_built_421_alignment(bytes, scope, &lanes)?;
        let form = match exact_legacy_as_built_421_solved_frame(bytes, records, scope) {
            Some(solved_frame) => DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, limits: Some(exact.limits) },
            None => DesignAssemblyAlignmentForm::LimitsOnly { limits: exact.limits },
        };
        return Some(DesignAssemblyAlignment { angle: exact.angle, offset: exact.offset, owners: exact.owners, form: Some(form) });
    }
    let (angle, offset, owners) = {
        if matches!(scope.frame_length, 671 | 744 | 748)
            && crate::design::assembly::operand_frame_variant(
                scope.frame_length,
                &scope.class_tag,
                &scope.paired_class_tag,
            )
            .is_none()
        {
            return None;
        }
        let (alignment_start, alignment_end) = crate::design::assembly::alignment_lane_bounds(
            scope.frame_length,
            &scope.class_tag,
            &scope.paired_class_tag,
            lanes.len(),
        )?;
        let alignment_lanes = lanes.get(alignment_start..alignment_end)?;
        let (angle, offset) = match alignment_lanes {
            [angle, offset_x, offset_y, offset_z] => (
                angle.evaluated_value,
                [offset_x.evaluated_value, offset_y.evaluated_value, offset_z.evaluated_value],
            ),
            [angle, axial_offset] => (angle.evaluated_value, [0.0, 0.0, axial_offset.evaluated_value]),
            _ => return None,
        };
        let owners: Vec<crate::records::Located<u32>> = alignment_lanes.iter()
            .map(|owner| crate::records::Located { value: owner.record_index, offset: owner.evaluated_value_offset }).collect();
        if legacy_class_388 {
            let owner_reference_order_matches = CLASS_388_OWNER_REFERENCE_ORDINALS
                .into_iter()
                .zip(lanes.iter())
                .all(|(scope_ordinal, owner)| {
                    scope.reference_members.values().nth(scope_ordinal) == Some(&owner.record_index)
                });
            if lanes
                .iter()
                .any(|owner| owner.class_tag != "282" || owner.frame_length != 103)
                || !owner_reference_order_matches
                || !scope.reference_members.values().skip(4).take(4).eq(owners.iter().map(|owner| &owner.value))
            {
                return None;
            }
        } else if legacy_class_383 {
            let owner_reference_order_matches = CLASS_383_OWNER_REFERENCE_ORDINALS
                .into_iter()
                .zip(lanes.iter())
                .all(|(scope_ordinal, owner)| {
                    scope.reference_members.values().nth(scope_ordinal) == Some(&owner.record_index)
                });
            if lanes
                .iter()
                .any(|owner| owner.class_tag != "284" || owner.frame_length != 103)
                || !owner_reference_order_matches
                || !scope.reference_members.values().skip(8).take(4).eq(owners.iter().map(|owner| &owner.value))
            {
                return None;
            }
        } else if crate::design::assembly::variable_reference_assembly_generation(
            &scope.class_tag,
            &scope.paired_class_tag,
        ) {
            if lanes
                .iter()
                .any(|owner| owner.class_tag != "289" || owner.frame_length != 103)
                || (0..scope.reference_members.len())
                    .filter(|&start| scope.reference_members.values_in(start..start + owners.len())
                        .is_some_and(|values| values.eq(owners.iter().map(|owner| &owner.value))))
                    .count()
                    != 1
            {
                return None;
            }
        } else if !scope.reference_members.values().rev().take(owners.len()).eq(owners.iter().map(|owner| &owner.value).rev()) {
            return None;
        }
        (angle, offset, owners)
    };
    let form = if scope.kind() == crate::records::DesignFeatureKind::AsBuilt {
        exact_assembly_operand_paths(bytes, records, scope).map(|paths| {
            match exact_as_built_operand_frames(bytes, &paths) {
                Some(frames) => DesignAssemblyAlignmentForm::qualified(frames,
                    paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })),
                None => DesignAssemblyAlignmentForm::UnframedPaths(paths),
            }
        })
    } else {
        exact_assembly_operand_frames(bytes, scope).map(|frames| {
            let qualifiers = if legacy_class_383 {
                exact_legacy_class_383_operand_paths(bytes, records, scope, &frames)
                    .map(|paths| paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }))
            } else if legacy_class_388 {
                exact_legacy_class_388_operand_paths(bytes, records, scope)
                    .map(|paths| paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }))
            } else if crate::design::assembly::variable_reference_assembly_generation(&scope.class_tag, &scope.paired_class_tag) {
                assembly_carrier_paths::exact_variable_reference_operand_qualifiers(bytes, records, scope, &frames)
                    .or_else(|| exact_assembly_operand_paths(bytes, records, scope)
                        .map(|paths| paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path })))
            } else {
                exact_assembly_operand_paths(bytes, records, scope)
                    .map(|paths| paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }))
            };
            match qualifiers {
                Some(qualifiers) => DesignAssemblyAlignmentForm::qualified(frames, qualifiers),
                None => DesignAssemblyAlignmentForm::Frames { frames },
            }
        })
    };
    Some(DesignAssemblyAlignment { angle, offset, owners, form })
}

pub(crate) fn exact_derived_instance_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Option<DesignDerivedInstanceConstruction> {
    if scope.kind() != crate::records::DesignFeatureKind::DerivedInstance
        || scope.class_tag != "279"
        || scope.paired_class_tag != "261"
        || scope.frame_length != derived_instance_279_261::LEN as u64
        || scope.reference_members.len() != 1
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if bytes.get(
        start + derived_instance_279_261::REFERENCE_MARKER
            ..start + derived_instance_279_261::REFERENCE_RECORD_INDEX,
    )? != [derived_instance_279_261::REFERENCE_MARKER_VALUE]
        || bytes.get(
            start + derived_instance_279_261::REFERENCE_RECORD_INDEX + 4
                ..start + derived_instance_279_261::REFERENCE_COUNT,
        )? != [0; 6]
        || View::u32_le_at(bytes, start + derived_instance_279_261::REFERENCE_COUNT)?
            != derived_instance_279_261::REFERENCE_COUNT_VALUE
        || marked_record_reference(bytes, start + derived_instance_279_261::RELATION_REFERENCE)?
            != *scope.reference_members.values().next()?
        || bytes.get(start + derived_instance_279_261::RELATION_REFERENCE + 11) != Some(&0)
    {
        return None;
    }
    let reference_record_index = View::u32_le_at(
        bytes,
        start + derived_instance_279_261::REFERENCE_RECORD_INDEX,
    )?;
    let transform_offset = start + derived_instance_279_261::TRANSFORM;
    let transform = rigid_transform_at(bytes, transform_offset)?;

    let relation_record_index = *scope.reference_members.values().next()?;
    let relation_at = records.first_at_or_after(0, relation_record_index)?;
    let (relation_kind, _) = lp_ascii_filtered(bytes, relation_at, 3..=3, u8::is_ascii_graphic)?;
    if relation_at >= start
        || relation_kind != "310"
        || next_indexed_record_offset(bytes, relation_at + 1)?
            != relation_at + derived_instance_relation_310::LEN
        || bytes.get(
            relation_at + derived_instance_relation_310::INDEXED_HEADER + 11
                ..relation_at + derived_instance_relation_310::CARRIER_MARKER,
        )? != [0; 10]
        || bytes.get(relation_at + derived_instance_relation_310::CARRIER_MARKER)
            != Some(&derived_instance_relation_310::CARRIER_MARKER_VALUE)
        || bytes.get(
            relation_at + derived_instance_relation_310::CARRIER_RECORD_INDEX + 4
                ..relation_at + derived_instance_relation_310::MIDDLE_MARKER,
        )? != [0; 8]
        || bytes.get(relation_at + derived_instance_relation_310::MIDDLE_MARKER)
            != Some(&derived_instance_relation_310::MIDDLE_MARKER_VALUE)
        || bytes.get(
            relation_at + derived_instance_relation_310::MIDDLE_RECORD_INDEX + 4
                ..relation_at + derived_instance_relation_310::SCOPE_MARKER,
        )? != [0; 7]
        || bytes.get(relation_at + derived_instance_relation_310::SCOPE_MARKER)
            != Some(&derived_instance_relation_310::SCOPE_MARKER_VALUE)
        || View::u32_le_at(
            bytes,
            relation_at + derived_instance_relation_310::SCOPE_RECORD_INDEX,
        )? != scope.record_index
        || bytes.get(
            relation_at + derived_instance_relation_310::SCOPE_RECORD_INDEX + 4
                ..relation_at + derived_instance_relation_310::LEN,
        )? != [0; 6]
    {
        return None;
    }
    let carrier_record_index = View::u32_le_at(
        bytes,
        relation_at + derived_instance_relation_310::CARRIER_RECORD_INDEX,
    )?;
    let stream = native_stream(&scope.id)?;
    let candidates = occurrences
        .iter()
        .filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.class_tag == "380"
                && occurrence.record_index == carrier_record_index
                && occurrence.byte_offset < relation_at as u64
                && occurrence.transform().map(|frame| frame.value) == Some(transform)
        })
        .collect::<Vec<_>>();
    let [carrier] = candidates.as_slice() else {
        return None;
    };
    Some(DesignDerivedInstanceConstruction {
        reference_record_index,
        relation_record_index,
        carrier_record_index,
        component_guid: carrier.component_guid.clone(),
        occurrence_guid: carrier.occurrence_guid.clone(),
        transform,
        transform_offset: u64::try_from(transform_offset).ok()?,
    })
}

pub(crate) fn exact_component_insert_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignComponentInsertConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let relation_record_index = *scope.reference_members.values().next()?;
    if scope.kind() != crate::records::DesignFeatureKind::ComponentInsert
        || scope.reference_members.len() != 1
    {
        return None;
    }
    let (transform, transform_at, occurrence_identity) =
        match (scope.frame_length, scope.paired_class_tag.as_str()) {
            (399, "259")
                if bytes.get(start + 11..start + 20)? == [0; 9]
                    && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                    && bytes.get(start + 33..start + 37)? == [0; 4]
                    && bytes.get(start + 37) == Some(&1)
                    && View::u32_le_at(bytes, start + 38)? == relation_record_index
                    && bytes.get(start + 42..start + 50)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
            {
                let transform_at = start + 50;
                (
                    rigid_transform_at(bytes, transform_at)?,
                    Some(transform_at),
                    View::u64_le_at(bytes, start + 25)?,
                )
            }
            (381, "261")
                if bytes.get(start + 11..start + 20)? == [0; 9]
                    && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                    && bytes.get(start + 33..start + 37)? == [0; 4]
                    && bytes.get(start + 37) == Some(&1)
                    && View::u32_le_at(bytes, start + 38)? == relation_record_index
                    && bytes.get(start + 42..start + 49)? == [0, 0, 0, 0, 0, 0, 1] =>
            {
                let transform_at = start + 49;
                (
                    rigid_transform_at(bytes, transform_at)?,
                    Some(transform_at),
                    View::u64_le_at(bytes, start + 25)?,
                )
            }
            (395, "258")
                if bytes.get(start + 11..start + 21)? == [0; 10]
                    && bytes.get(start + 29..start + 33)? == [0; 4]
                    && bytes.get(start + 33) == Some(&1)
                    && View::u32_le_at(bytes, start + 34)? == relation_record_index
                    && bytes.get(start + 38..start + 46)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
            {
                let transform_at = start + 46;
                (
                    rigid_transform_at(bytes, transform_at)?,
                    Some(transform_at),
                    View::u64_le_at(bytes, start + 21)?,
                )
            }
            (404, _)
                if bytes.get(start + 11..start + 20)? == [0; 9]
                    && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                    && bytes.get(start + 25..start + 29)? == [0; 4]
                    && bytes.get(start + 37..start + 41)? == [0; 4]
                    && bytes.get(start + 41) == Some(&1)
                    && View::u32_le_at(bytes, start + 42)? == relation_record_index
                    && bytes.get(start + 46..start + 52)? == [0; 6]
                    && bytes.get(start + 52..start + 54)? == [1, 0] =>
            {
                let transform_at = start + 54;
                (
                    rigid_transform_at(bytes, transform_at)?,
                    Some(transform_at),
                    View::u64_le_at(bytes, start + 29)?,
                )
            }
            (261, "263") if scope.class_tag == "296" => (
                identity_matrix(),
                None,
                exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
            ),
            (261, "261") if scope.class_tag == "410" => (
                identity_matrix(),
                None,
                exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
            ),
            (261, "258") if scope.class_tag == "426" => (
                identity_matrix(),
                None,
                exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
            ),
            (261, "266") if scope.class_tag == "434" => (
                identity_matrix(),
                None,
                exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
            ),
            (261, "264") if scope.class_tag == "414" => (
                identity_matrix(),
                None,
                exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
            ),
            (257 | 267, "264") if scope.class_tag == "414" => (
                identity_matrix(),
                None,
                exact_component_insert_identity_scope_shifted(bytes, start, relation_record_index)?,
            ),
            (389, "264") if scope.class_tag == "414" => {
                exact_component_insert_scope_414_264_389(bytes, start, relation_record_index)?
            }
            (257, "262") if scope.class_tag == "283" => {
                exact_component_insert_scope_283_262_257(bytes, start, relation_record_index)?
            }
            (385, "262") if scope.class_tag == "283" => {
                exact_component_insert_scope_283_262_385(bytes, start, relation_record_index)?
            }
            _ => return None,
        };
    let relation_at = records.first_at_or_after(0, relation_record_index)?;
    let (carrier_record_index, placements) = if scope.frame_length == 404 {
        if relation_at >= start
            || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 58
            || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
            || bytes.get(relation_at + 21) != Some(&1)
            || bytes.get(relation_at + 26..relation_at + 32)? != [0; 6]
            || bytes.get(relation_at + 32..relation_at + 35)? != [1, 0, 0]
            || bytes.get(relation_at + 35) != Some(&1)
            || bytes.get(relation_at + 40..relation_at + 47)? != [0; 7]
            || bytes.get(relation_at + 47) != Some(&1)
            || View::u32_le_at(bytes, relation_at + 48)? != scope.record_index
            || bytes.get(relation_at + 52..relation_at + 58)? != [0; 6]
        {
            return None;
        }
        let carrier_record_index = View::u32_le_at(bytes, relation_at + 22)?;
        let mut placements = Vec::new();
        for &carrier_at in records
            .offsets(carrier_record_index)
            .iter()
            .filter(|at| **at < relation_at)
        {
            for at in carrier_at + 11..relation_at {
                let Some((role, after_role)) = lp_utf16_bounded(bytes, at, 36..=36) else {
                    continue;
                };
                if !crate::bytes::is_guid_relaxed(&role)
                    || bytes.get(after_role..after_role + 12)
                        != Some(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                {
                    continue;
                }
                for transform_at in carrier_at + 11..at {
                    if rigid_transform_at(bytes, transform_at) == Some(transform) {
                        placements.push((role.clone(), at + 4, Some(transform_at)));
                    }
                }
            }
        }
        (carrier_record_index, placements)
    } else if scope.class_tag == "426" && scope.paired_class_tag == "258" {
        exact_component_insert_class_426_relation(
            bytes,
            records,
            relation_at,
            start,
            relation_record_index,
            scope.record_index,
        )?
    } else {
        if relation_at >= start
            || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 57
            || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
            || bytes.get(relation_at + 21) != Some(&1)
            || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
            || bytes.get(relation_at + 34) != Some(&1)
            || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
            || bytes.get(relation_at + 46) != Some(&1)
            || View::u32_le_at(bytes, relation_at + 47)? != scope.record_index
            || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
        {
            return None;
        }
        let carrier_record_index = View::u32_le_at(bytes, relation_at + 22)?;
        let carrier_at = unique_indexed_record_before(records, carrier_record_index, relation_at)?;
        if scope.class_tag == "283" && scope.paired_class_tag == "262" {
            let (role, role_offset) = exact_component_insert_carrier_334(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            )?;
            (carrier_record_index, vec![(role, role_offset, None)])
        } else if scope.class_tag == "296" && scope.paired_class_tag == "263" {
            let (role, role_offset) = crate::xref::grouped_component_insert_identity(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            )?;
            (carrier_record_index, vec![(role, role_offset, None)])
        } else if scope.class_tag == "410" && scope.paired_class_tag == "261" {
            let (role, role_offset) = crate::xref::grouped_component_insert_identity_class380(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            )?;
            (carrier_record_index, vec![(role, role_offset, None)])
        } else if scope.class_tag == "434" && scope.paired_class_tag == "266" {
            let (role, role_offset) = crate::xref::grouped_component_insert_identity_class341(
                bytes,
                carrier_at,
                relation_at,
                carrier_record_index,
            )?;
            (carrier_record_index, vec![(role, role_offset, None)])
        } else if scope.class_tag == "414" && scope.paired_class_tag == "264" {
            let (role, role_offset, carrier_transform_offset) =
                crate::xref::repeated_target_component_insert(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                    transform,
                )?;
            (
                carrier_record_index,
                vec![(role, role_offset, carrier_transform_offset)],
            )
        } else {
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
                    placements.push((role, at + 4, Some(transform_at)));
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
            (carrier_record_index, placements)
        }
    };
    let [(neutron_role, neutron_role_offset, carrier_transform_offset)] = placements.as_slice()
    else {
        return None;
    };
    Some(DesignComponentInsertConstruction {
        relation_record_index,
        carrier_record_index,
        occurrence_identity: Some(occurrence_identity),
        neutron_role: neutron_role.clone(),
        neutron_role_offset: u64::try_from(*neutron_role_offset).ok()?,
        placement: match (transform_at, *carrier_transform_offset) {
            (Some(offset), carrier_offset) => Some(crate::records::DesignComponentInsertMatrix {
                scope: crate::records::Located { value: transform, offset: u64::try_from(offset).ok()? },
                carrier_offset: carrier_offset.map(u64::try_from).transpose().ok()?,
            }),
            (None, None) => None,
            (None, Some(_)) => return None,
        },
    })
}

type ComponentInsertClass426Relation = (u32, Vec<(String, usize, Option<usize>)>);

fn exact_component_insert_class_426_relation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    relation_at: usize,
    scope_at: usize,
    relation_record_index: u32,
    scope_record_index: u32,
) -> Option<ComponentInsertClass426Relation> {
    let relation_end = relation_at + component_insert_relation_345::LEN;
    let (relation_class, relation_after_tag) =
        lp_ascii_filtered(bytes, relation_at, 3..=3, u8::is_ascii_digit)?;
    if relation_class != "345"
        || relation_after_tag != relation_at + 7
        || View::u32_le_at(bytes, relation_after_tag)? != relation_record_index
        || relation_at >= scope_at
        || next_indexed_record_offset(bytes, relation_at + 1)? != relation_end
        || bytes.get(
            relation_at + component_insert_relation_345::INDEXED_HEADER + 11
                ..relation_at + component_insert_relation_345::FIRST_MARKER,
        )? != [0; 10]
        || bytes.get(relation_at + component_insert_relation_345::FIRST_MARKER)
            != Some(&component_insert_relation_345::FIRST_MARKER_VALUE)
        || bytes.get(
            relation_at + component_insert_relation_345::FIRST_CARRIER_RECORD_INDEX + 4
                ..relation_at + component_insert_relation_345::SECOND_MARKER,
        )? != [0; 8]
        || bytes.get(relation_at + component_insert_relation_345::SECOND_MARKER)
            != Some(&component_insert_relation_345::SECOND_MARKER_VALUE)
        || bytes.get(
            relation_at + component_insert_relation_345::SECOND_CHILD_RECORD_INDEX + 4
                ..relation_at + component_insert_relation_345::SCOPE_MARKER,
        )? != [0; 7]
        || bytes.get(relation_at + component_insert_relation_345::SCOPE_MARKER)
            != Some(&component_insert_relation_345::SCOPE_MARKER_VALUE)
        || View::u32_le_at(
            bytes,
            relation_at + component_insert_relation_345::SCOPE_RECORD_INDEX,
        )? != scope_record_index
        || bytes.get(
            relation_at + component_insert_relation_345::SCOPE_RECORD_INDEX + 4..relation_end,
        )? != [0; 6]
    {
        return None;
    }

    let paired_at = relation_end;
    let (paired_class, paired_after_tag) =
        lp_ascii_filtered(bytes, paired_at, 3..=3, u8::is_ascii_digit)?;
    if paired_class != "258"
        || paired_after_tag != paired_at + 7
        || View::u32_le_at(bytes, paired_after_tag)? != relation_record_index
    {
        return None;
    }

    let carrier_record_index = View::u32_le_at(
        bytes,
        relation_at + component_insert_relation_345::FIRST_CARRIER_RECORD_INDEX,
    )?;
    let child_record_index = View::u32_le_at(
        bytes,
        relation_at + component_insert_relation_345::SECOND_CHILD_RECORD_INDEX,
    )?;
    let child_at = records.first_at_or_after(paired_at + 11, child_record_index)?;
    let child_end = child_at + component_insert_relation_child_393::LEN;
    let (child_class, child_after_tag) =
        lp_ascii_filtered(bytes, child_at, 3..=3, u8::is_ascii_digit)?;
    if child_class != "393"
        || child_after_tag != child_at + 7
        || View::u32_le_at(bytes, child_after_tag)? != child_record_index
        || next_indexed_record_offset(bytes, paired_at + 1)? != child_at
        || next_indexed_record_offset(bytes, child_at + 1)? != child_end
        || child_end != scope_at
        || bytes
            .get(child_at + 11..child_at + component_insert_relation_child_393::RELATION_MARKER)?
            != [0; 20]
        || bytes.get(child_at + component_insert_relation_child_393::RELATION_MARKER)
            != Some(&component_insert_relation_child_393::RELATION_MARKER_VALUE)
        || View::u32_le_at(
            bytes,
            child_at + component_insert_relation_child_393::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(
            child_at + component_insert_relation_child_393::RELATION_RECORD_INDEX + 4
                ..child_at + component_insert_relation_child_393::OPAQUE_TOKEN,
        )? != [0; 6]
        || View::u64_le_at(
            bytes,
            child_at + component_insert_relation_child_393::OPAQUE_TOKEN,
        )
        .is_none()
        || bytes.get(child_at + component_insert_relation_child_393::OPAQUE_TOKEN + 8..child_end)?
            != [0; 8]
    {
        return None;
    }

    let carrier_at = unique_indexed_record_before(records, carrier_record_index, relation_at)?;
    let (role, role_offset) = crate::xref::grouped_component_insert_identity_class369(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
    )?;
    Some((carrier_record_index, vec![(role, role_offset, None)]))
}

fn exact_component_insert_carrier_334(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    if class_tag != "334"
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag)? != carrier_record_index
    {
        return None;
    }
    let (component_identity, _) = lp_utf16_bounded(
        bytes,
        carrier_at + component_carrier_334::COMPONENT_IDENTITY,
        36..=36,
    )?;
    if !crate::bytes::is_guid_relaxed(&component_identity) {
        return None;
    }

    let role_start = carrier_at + component_carrier_334::NEUTRON_ROLE;
    let (role, role_end) = direct_utf16_role_until_tail(bytes, role_start, relation_at)?;
    if !crate::bytes::is_guid_prefix(&role)
        || role.as_bytes().get(36) != Some(&b'_')
        || !role.get(37..)?.starts_with("urn:")
        || bytes.get(role_end)? != &0
        || bytes.get(role_end + 1)? == &0
        || bytes.get(role_end + 2..role_end + 6)? != [0; 4]
        || View::u32_le_at(bytes, role_end + 6)? == 0
    {
        return None;
    }
    let (following_identity, _) =
        lp_utf16_bounded(bytes, role_end + COMPONENT_CARRIER_ROLE_TAIL_BYTES, 36..=36)?;
    crate::bytes::is_guid_relaxed(&following_identity).then_some((role, role_start))
}

const COMPONENT_CARRIER_ROLE_TAIL_BYTES: usize = 10;

fn direct_utf16_role_until_tail(
    bytes: &[u8],
    start: usize,
    limit: usize,
) -> Option<(String, usize)> {
    let mut role = String::new();
    let mut at = start;
    while at.checked_add(COMPONENT_CARRIER_ROLE_TAIL_BYTES)? <= limit {
        if bytes.get(at)? == &0
            && bytes.get(at + 2..at + 6)? == [0; 4]
            && View::u32_le_at(bytes, at + 6).is_some_and(|value| value != 0)
        {
            return Some((role, at));
        }
        let code_unit = View::u16_le_at(bytes, at)?;
        let byte = u8::try_from(code_unit).ok()?;
        if !byte.is_ascii_graphic() {
            return None;
        }
        role.push(char::from(byte));
        at = at.checked_add(2)?;
    }
    None
}

fn exact_component_insert_scope_283_262_257(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<([[f64; 4]; 4], Option<usize>, u64)> {
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(
            start + component_scope_283_257::RELATION_MARKER
                ..start + component_scope_283_257::RELATION_MARKER + 1,
        )? != [1]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 38..start + 44)? != [0; 6]
        || bytes.get(start + 44..start + 46)? != [1, 1]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::NULL_GUID_CODE_UNIT_COUNT,
        )? != 36
    {
        return None;
    }
    let (null_guid, after_null_guid) = lp_utf16_bounded(
        bytes,
        start + component_scope_283_257::NULL_GUID_CODE_UNIT_COUNT,
        36..=36,
    )?;
    if null_guid != NULL_COMPONENT_INSERT_GUID
        || after_null_guid != start + component_scope_283_257::REFERENCE_COUNT - 3
        || View::u32_le_at(bytes, start + component_scope_283_257::REFERENCE_COUNT)? != 1
        || bytes.get(start + component_scope_283_257::REFERENCE_MARKER) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::REFERENCE_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 134..start + 140)? != [0; 6]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::PREVIOUS_HISTORY_STATE_ID,
        )? != u32::MAX
    {
        return None;
    }
    Some((
        identity_matrix(),
        None,
        View::u64_le_at(bytes, start + component_scope_283_257::OCCURRENCE_IDENTITY)?,
    ))
}

fn exact_component_insert_scope_283_262_385(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<([[f64; 4]; 4], Option<usize>, u64)> {
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 44..start + 52)? != [1, 0, 0, 0, 0, 0, 0, 0]
        || bytes.get(start + 38..start + 44)? != [0; 6]
        || bytes.get(
            start + component_scope_283_385::RELATION_MARKER
                ..start + component_scope_283_385::RELATION_MARKER + 1,
        )? != [1]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_385::RELATION_RECORD_INDEX,
        )? != relation_record_index
    {
        return None;
    }
    let transform_at = start + component_scope_283_385::TRANSFORM;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let (null_guid, after_null_guid) = lp_utf16_bounded(
        bytes,
        start + component_scope_283_385::NULL_GUID_CODE_UNIT_COUNT,
        36..=36,
    )?;
    if null_guid != NULL_COMPONENT_INSERT_GUID
        || after_null_guid != start + component_scope_283_385::REFERENCE_COUNT - 3
        || View::u32_le_at(bytes, start + component_scope_283_385::REFERENCE_COUNT)? != 1
        || bytes.get(start + component_scope_283_385::REFERENCE_MARKER) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_scope_283_385::REFERENCE_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 262..start + 268)? != [0; 6]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_385::PREVIOUS_HISTORY_STATE_ID,
        )? != u32::MAX
    {
        return None;
    }
    Some((
        transform,
        Some(transform_at),
        View::u64_le_at(bytes, start + component_scope_283_385::OCCURRENCE_IDENTITY)?,
    ))
}

const NULL_COMPONENT_INSERT_GUID: &str = "00000000-0000-0000-0000-000000000000";

fn exact_component_insert_identity_scope(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<u64> {
    const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
    if bytes.get(start + 11..start + 20)? != [0; 9]
        || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || bytes.get(start + 33..start + 37)? != [0; 4]
        || bytes.get(start + 37) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_identity_scope::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 42..start + 48)? != [0; 6]
        || bytes.get(
            start + component_identity_scope::IDENTITY_MARKERS
                ..start + component_identity_scope::IDENTITY_MARKERS + 2,
        )? != [1, 1]
        || View::u32_le_at(
            bytes,
            start + component_identity_scope::OPAQUE_CODE_UNIT_COUNT,
        )? != 36
    {
        return None;
    }
    let (opaque_guid, after_opaque_guid) = lp_utf16_bounded(
        bytes,
        start + component_identity_scope::OPAQUE_CODE_UNIT_COUNT,
        36..=36,
    )?;
    if opaque_guid != NULL_GUID
        || after_opaque_guid != start + component_identity_scope::OPAQUE_UTF16_PAYLOAD + 72
    {
        return None;
    }
    View::u64_le_at(bytes, start + component_identity_scope::OCCURRENCE_IDENTITY)
}

fn exact_component_insert_identity_scope_shifted(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<u64> {
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 29..start + 33)? != [0; 4]
        || bytes.get(start + component_identity_shifted::RELATION_MARKER) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_identity_shifted::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 38..start + 44)? != [0; 6]
        || bytes.get(
            start + component_identity_shifted::IDENTITY_MARKERS
                ..start + component_identity_shifted::IDENTITY_MARKERS + 2,
        )? != [1, 1]
    {
        return None;
    }
    let (null_guid, after_null_guid) = lp_utf16_bounded(
        bytes,
        start + component_identity_shifted::NULL_GUID_CODE_UNIT_COUNT,
        36..=36,
    )?;
    if null_guid != NULL_COMPONENT_INSERT_GUID
        || after_null_guid != start + component_identity_shifted::LEN
    {
        return None;
    }
    View::u64_le_at(
        bytes,
        start + component_identity_shifted::OCCURRENCE_IDENTITY,
    )
}

fn exact_component_insert_scope_414_264_389(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<([[f64; 4]; 4], Option<usize>, u64)> {
    if bytes.get(start + 11..start + 20)? != [0; 9]
        || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || bytes.get(start + 33..start + 37)? != [0; 4]
        || bytes.get(start + component_matrix_414::RELATION_MARKER) != Some(&1)
        || View::u32_le_at(bytes, start + component_matrix_414::RELATION_RECORD_INDEX)?
            != relation_record_index
        || bytes.get(start + 42..start + 48)? != [0; 6]
        || bytes.get(
            start + component_matrix_414::MATRIX_MARKERS
                ..start + component_matrix_414::MATRIX_MARKERS + 2,
        )? != [1, 0]
    {
        return None;
    }
    let transform_at = start + component_matrix_414::TRANSFORM;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let (null_guid, after_null_guid) = lp_utf16_bounded(
        bytes,
        start + component_matrix_414::NULL_GUID_CODE_UNIT_COUNT,
        36..=36,
    )?;
    if null_guid != NULL_COMPONENT_INSERT_GUID
        || after_null_guid != start + component_matrix_414::LEN
    {
        return None;
    }
    Some((
        transform,
        Some(transform_at),
        View::u64_le_at(bytes, start + component_matrix_414::OCCURRENCE_IDENTITY)?,
    ))
}

fn legacy_component_insert_placements(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    transform: [[f64; 4]; 4],
) -> Vec<(String, usize, Option<usize>)> {
    let Some((class_tag, after_tag)) =
        lp_ascii_filtered(bytes, carrier_at, 3..=3, u8::is_ascii_digit)
    else {
        return Vec::new();
    };
    if class_tag != "288"
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
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
            placements.push((role, role_at + 4, Some(carrier_transform_at)));
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
    let relation_record_index = *scope.reference_members.values().next()?;
    // The compact frame omits one four-byte prologue field, so both placements
    // and every marked reference before them move four bytes earlier.
    let source_at = match (scope.kind_name(), scope.frame_length) {
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
        || View::u32_le_at(bytes, relation_at + 47)? != scope.record_index
        || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
    {
        return None;
    }
    let copied_occurrence_record_index = View::u32_le_at(bytes, relation_at + 22)?;
    let copied_candidates = occurrences
        .iter()
        .filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.record_index == copied_occurrence_record_index
                && occurrence.byte_offset < relation_at as u64
                && occurrence.transform().map(|frame| frame.value) == Some(copied_transform)
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
                && occurrence.transform().is_none()
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
    let Some(stream) = native_stream(&scope.id).map(str::to_owned) else {
        return;
    };
    let byte_offset = scope.byte_offset;
    let Some(instances) = scope
        .rectangular_pattern_construction_mut()
        .and_then(|construction| construction.instances.as_mut())
    else {
        return;
    };
    let mut generated = Vec::new();
    for (ordinal, frame) in instances.frames().enumerate().skip(1) {
        let candidates = occurrences
            .iter()
            .filter(|occurrence| {
                native_stream(&occurrence.id) == Some(stream.as_str())
                    && occurrence.transform().map(|frame| frame.offset) == Some(frame.transform.offset)
                    && occurrence.occurrence_ordinal() == ordinal as u32 + 1
            })
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return;
        };
        generated.push((*candidate, *frame));
    }
    let Some(component_guid) = generated
        .first()
        .map(|(occurrence, _)| &occurrence.component_guid)
    else {
        return;
    };
    if generated.iter().any(|(occurrence, _)| {
        !occurrence
            .component_guid
            .eq_ignore_ascii_case(component_guid)
    }) {
        return;
    }
    let seed_candidates = occurrences
        .iter()
        .filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream.as_str())
                && occurrence.byte_offset < byte_offset
                && occurrence
                    .component_guid
                    .eq_ignore_ascii_case(component_guid)
                && matches!(occurrence.placement, crate::records::DesignComponentOccurrencePlacement::Base)
        })
        .collect::<Vec<_>>();
    let [seed] = seed_candidates.as_slice() else {
        return;
    };
    let Some(seed_frame) = instances.frames().next().copied() else {
        return;
    };
    *instances = DesignRectangularPatternInstances::Components {
        component_guid: component_guid.clone(),
        seed: crate::records::DesignPatternComponentInstance { instance: seed_frame, occurrence_guid: seed.occurrence_guid.clone() },
        generated: generated.into_iter().map(|(occurrence, instance)| crate::records::DesignPatternComponentInstance { instance, occurrence_guid: occurrence.occurrence_guid.clone() }).collect(),
    };
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
    let start = usize::try_from(scope.byte_offset).ok()?;
    let frame_variant = crate::design::assembly::operand_frame_variant(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    let frame_offsets = match frame_variant {
        crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388 => (
            class_388_assemble::FIRST_OPERAND_REFERENCE,
            class_388_assemble::FIRST_OPERAND_TRANSFORM,
            class_388_assemble::SECOND_OPERAND_REFERENCE,
            class_388_assemble::SECOND_OPERAND_TRANSFORM,
        ),
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
            if scope.class_tag == "383" && scope.paired_class_tag == "258" =>
        {
            (
                class_383_scope::FIRST_OPERAND_REFERENCE,
                class_383_scope::FIRST_OPERAND_TRANSFORM,
                class_383_scope::SECOND_OPERAND_REFERENCE,
                class_383_scope::SECOND_OPERAND_TRANSFORM,
            )
        }
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
            if scope.class_tag == "406" && scope.paired_class_tag == "261" =>
        {
            (
                class_406_assemble::FIRST_OPERAND_REFERENCE,
                class_406_assemble::FIRST_OPERAND_TRANSFORM,
                class_406_assemble::SECOND_OPERAND_REFERENCE,
                class_406_assemble::SECOND_OPERAND_TRANSFORM,
            )
        }
        crate::design::assembly::AssemblyOperandFrameVariant::Standard => (28, 40, 168, 180),
        crate::design::assembly::AssemblyOperandFrameVariant::Compact => (24, 36, 164, 176),
        crate::design::assembly::AssemblyOperandFrameVariant::Axial => (28, 39, 167, 178),
    };
    if usize::try_from(scope.paired_byte_offset).ok()?
        != start.checked_add(usize::try_from(scope.frame_length).ok()?)?
        || bytes.get(start + 11..start + 20)? != [0; 9]
    {
        return None;
    }
    if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388
    ) {
        exact_legacy_class_388_scope(bytes, scope)?;
        if bytes.get(start + 11..start + class_388_assemble::SCOPE_FLAGS)?
            != [0; class_388_assemble::SCOPE_FLAGS - 11]
            || bytes.get(
                start + class_388_assemble::SCOPE_FLAGS
                    ..start + class_388_assemble::SCOPE_FLAGS + 6,
            )? != class_388_assemble::SCOPE_FLAGS_VALUE
            || bytes.get(start + 26..start + class_388_assemble::FIRST_OPERAND_REFERENCE)? != [0; 2]
            || bytes.get(start + 39) != Some(&0)
            || bytes.get(start + 168 + 11..start + class_388_assemble::SECOND_OPERAND_TRANSFORM)?
                != [0; 1]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
    ) {
        let standard_tail_marker_offset = if scope.frame_length == class_383_scope::LEN as u64
            && scope.class_tag == "383"
            && scope.paired_class_tag == "258"
        {
            class_383_scope::STANDARD_TAIL_MARKER
        } else {
            308
        };
        if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
            || !matches!(bytes.get(start + 25), Some(0 | 1))
            || bytes.get(start + 26..start + 28)? != [0; 2]
            || bytes.get(start + 33..start + 40)? != [0; 7]
            || bytes.get(start + 173..start + 180)? != [0; 7]
            || !crate::design::assembly::variable_reference_assembly_generation(
                &scope.class_tag,
                &scope.paired_class_tag,
            ) && bytes
                .get(start + standard_tail_marker_offset..start + standard_tail_marker_offset + 4)?
                != [0; 4]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Compact
    ) {
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

fn exact_legacy_class_388_scope(bytes: &[u8], scope: &DesignParameterScope) -> Option<()> {
    if scope.class_tag != "388"
        || scope.paired_class_tag != "266"
        || scope.frame_length != class_388_assemble::LEN as u64
        || scope.reference_members.len() != class_388_assemble::REFERENCE_COUNT_VALUE as usize
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let paired = usize::try_from(scope.paired_byte_offset).ok()?;
    let zero_prefix = bytes.get(start + 11..start + class_388_assemble::SCOPE_FLAGS)?;
    let scope_flags = bytes.get(
        start + class_388_assemble::SCOPE_FLAGS..start + class_388_assemble::SCOPE_FLAGS + 6,
    )?;
    let zero_operand_prefix =
        bytes.get(start + 26..start + class_388_assemble::FIRST_OPERAND_REFERENCE)?;
    let first_separator = bytes.get(start + 39);
    let second_separator = bytes.get(start + 179);
    let operand_path_locator_count = View::u32_le_at(
        bytes,
        start + class_388_assemble::OPERAND_PATH_LOCATOR_COUNT,
    )?;
    let reference_trailer = bytes.get(
        start + class_388_assemble::REFERENCE_TRAILER
            ..start + class_388_assemble::REFERENCE_TRAILER + 4,
    )?;
    let kind_code_unit_count =
        View::u32_le_at(bytes, start + class_388_assemble::KIND_CODE_UNIT_COUNT)?;
    if paired != start.checked_add(class_388_assemble::LEN)? {
        return None;
    }
    if zero_prefix != [0; class_388_assemble::SCOPE_FLAGS - 11] {
        return None;
    }
    if scope_flags != class_388_assemble::SCOPE_FLAGS_VALUE {
        return None;
    }
    if zero_operand_prefix != [0; 2] {
        return None;
    }
    if first_separator != Some(&0) {
        return None;
    }
    if second_separator != Some(&0) {
        return None;
    }
    if operand_path_locator_count != class_388_assemble::OPERAND_PATH_LOCATOR_COUNT_VALUE {
        return None;
    }
    if reference_trailer != class_388_assemble::REFERENCE_TRAILER_VALUE {
        return None;
    }
    if kind_code_unit_count != class_388_assemble::KIND_CODE_UNIT_COUNT_VALUE {
        return None;
    }
    let operand_path_locator_references = [
        start + class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES,
        start + class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES + 11,
    ];
    let [Some(first_locator), Some(second_locator)] =
        operand_path_locator_references.map(|at| marked_record_reference(bytes, at))
    else {
        return None;
    };
    if first_locator == 0 || second_locator == 0 || first_locator == second_locator {
        return None;
    }
    let external_component = marked_record_reference(
        bytes,
        start + class_388_assemble::EXTERNAL_COMPONENT_REFERENCE,
    )?;
    if external_component == 0 {
        return None;
    }
    let (component_identity, identity_end) = lp_utf16_bounded(
        bytes,
        start + class_388_assemble::COMPONENT_IDENTITY,
        36..=36,
    )?;
    if !is_guid_relaxed(&component_identity)
        || identity_end != start + class_388_assemble::COMPONENT_IDENTITY + 76
    {
        return None;
    }
    let (kind, kind_end) = lp_utf16_bounded(
        bytes,
        start + class_388_assemble::KIND_CODE_UNIT_COUNT,
        class_388_assemble::KIND_CODE_UNIT_COUNT_VALUE as usize
            ..=class_388_assemble::KIND_CODE_UNIT_COUNT_VALUE as usize,
    )?;
    if kind != "Assemble"
        || kind_end != start + class_388_assemble::FEATURE_ORDINAL
        || View::u32_le_at(bytes, start + class_388_assemble::FEATURE_ORDINAL)?
            != scope.feature_ordinal.get()
    {
        return None;
    }
    for (ordinal, record_index) in scope.reference_members.values().enumerate() {
        let at = start
            .checked_add(class_388_assemble::REFERENCE_ENTRIES)?
            .checked_add(ordinal.checked_mul(ASSEMBLY_MARKED_REFERENCE_LEN)?)?;
        if marked_record_reference(bytes, at) != Some(*record_index) {
            return None;
        }
    }
    Some(())
}

const ASSEMBLY_MARKED_REFERENCE_LEN: usize = 11;
const CLASS_388_OWNER_REFERENCE_ORDINALS: [usize; 28] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 30, 31,
    32, 33,
];
const CLASS_383_OWNER_REFERENCE_ORDINALS: [usize; 20] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 20, 21, 22, 23, 33, 34, 35, 36,
];

#[derive(Clone, Copy)]
struct LegacyClass383OperandSpec {
    leading_ordinal: usize,
    leading_identity_ordinal: usize,
    child_ordinal: usize,
    child_identity_ordinal: usize,
    first_face_ordinal: usize,
    first_face_identity_ordinal: usize,
    second_face_ordinal: usize,
    second_face_identity_ordinal: usize,
    placement_owner_start: usize,
    carrier_ordinal: usize,
    scope_operand_reference_offset: usize,
}

const CLASS_383_OPERAND_SPECS: [LegacyClass383OperandSpec; 2] = [
    LegacyClass383OperandSpec {
        leading_ordinal: 12,
        leading_identity_ordinal: 13,
        child_ordinal: 14,
        child_identity_ordinal: 15,
        first_face_ordinal: 16,
        first_face_identity_ordinal: 17,
        second_face_ordinal: 18,
        second_face_identity_ordinal: 19,
        placement_owner_start: 20,
        carrier_ordinal: 24,
        scope_operand_reference_offset: class_383_scope::FIRST_OPERAND_REFERENCE,
    },
    LegacyClass383OperandSpec {
        leading_ordinal: 25,
        leading_identity_ordinal: 26,
        child_ordinal: 27,
        child_identity_ordinal: 28,
        first_face_ordinal: 29,
        first_face_identity_ordinal: 30,
        second_face_ordinal: 31,
        second_face_identity_ordinal: 32,
        placement_owner_start: 33,
        carrier_ordinal: 37,
        scope_operand_reference_offset: class_383_scope::SECOND_OPERAND_REFERENCE,
    },
];

fn exact_legacy_class_383_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frames: &[DesignAssemblyOperandFrame; 2],
) -> Option<[DesignAssemblyOperandPath; 2]> {
    if !crate::design::assembly::legacy_class_383_258_scope(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    ) || scope.reference_members.len() != 38
    {
        return None;
    }
    CLASS_383_OPERAND_SPECS
        .into_iter()
        .enumerate()
        .map(|(ordinal, spec)| {
            exact_legacy_class_383_operand_path(bytes, records, scope, &frames[ordinal], spec)
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn exact_legacy_class_383_operand_path(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    frame: &DesignAssemblyOperandFrame,
    spec: LegacyClass383OperandSpec,
) -> Option<DesignAssemblyOperandPath> {
    let member = |ordinal| scope.reference_members.values().nth(ordinal).copied();
    let leading_record_index = member(spec.leading_ordinal)?;
    let leading_identity_record_index = member(spec.leading_identity_ordinal)?;
    let child_record_index = member(spec.child_ordinal)?;
    let child_identity_record_index = member(spec.child_identity_ordinal)?;
    let first_face_record_index = member(spec.first_face_ordinal)?;
    let first_face_identity_record_index = member(spec.first_face_identity_ordinal)?;
    let second_face_record_index = member(spec.second_face_ordinal)?;
    let second_face_identity_record_index = member(spec.second_face_identity_ordinal)?;
    let carrier_record_index = member(spec.carrier_ordinal)?;
    let placement_owners = (0..4)
        .map(|ordinal| member(spec.placement_owner_start.checked_add(ordinal)?))
        .collect::<Option<Vec<_>>>()?;
    let (leading_at, leading_paired_at) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        leading_record_index,
        "387",
        class_383_leading::LEN,
    )?;
    let (leading_identity_at, _) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        leading_identity_record_index,
        "359",
        class_383_identity::LEN,
    )?;
    let (child_at, child_paired_at) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        child_record_index,
        "387",
        class_383_child::LEN,
    )?;
    let (child_identity_at, _) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        child_identity_record_index,
        "359",
        class_383_identity::LEN,
    )?;
    let (first_face_at, _) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        first_face_record_index,
        "394",
        class_383_face::LEN,
    )?;
    let (first_face_identity_at, _) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        first_face_identity_record_index,
        "359",
        class_383_identity::LEN,
    )?;
    let (second_face_at, _) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        second_face_record_index,
        "394",
        class_383_face::LEN,
    )?;
    let (second_face_identity_at, _) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        second_face_identity_record_index,
        "359",
        class_383_identity::LEN,
    )?;
    let (carrier_at, carrier_paired_at) = exact_legacy_class_383_record_frame(
        bytes,
        records,
        carrier_record_index,
        "378",
        class_383_carrier::LEN,
    )?;
    let structural_checks = [
        leading_paired_at == leading_at.checked_add(class_383_leading::LEN)?,
        child_paired_at == child_at.checked_add(class_383_child::LEN)?,
        marked_record_reference(
            bytes,
            leading_at.checked_add(class_383_leading::IDENTITY_REFERENCE)?,
        ) == Some(leading_identity_record_index),
        marked_record_reference(
            bytes,
            leading_at.checked_add(class_383_leading::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            child_at.checked_add(class_383_child::IDENTITY_REFERENCE)?,
        ) == Some(child_identity_record_index),
        marked_record_reference(
            bytes,
            child_at.checked_add(class_383_child::LEADING_REFERENCE)?,
        ) == Some(leading_record_index),
        marked_record_reference(
            bytes,
            child_at.checked_add(class_383_child::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            first_face_at.checked_add(class_383_face::IDENTITY_REFERENCE)?,
        ) == Some(first_face_identity_record_index),
        marked_record_reference(
            bytes,
            first_face_at.checked_add(class_383_face::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            second_face_at.checked_add(class_383_face::IDENTITY_REFERENCE)?,
        ) == Some(second_face_identity_record_index),
        marked_record_reference(
            bytes,
            second_face_at.checked_add(class_383_face::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            leading_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            child_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            first_face_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            second_face_identity_at.checked_add(class_383_identity::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::CHILD_REFERENCE)?,
        ) == Some(child_record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::SECOND_FACE_REFERENCE)?,
        ) == Some(second_face_record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::FIRST_FACE_REFERENCE)?,
        ) == Some(first_face_record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::REPEATED_CHILD_REFERENCE)?,
        ) == Some(child_record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::REPEATED_FIRST_FACE_REFERENCE)?,
        ) == Some(first_face_record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::REPEATED_SECOND_FACE_REFERENCE)?,
        ) == Some(second_face_record_index),
        marked_record_reference(
            bytes,
            carrier_at.checked_add(class_383_carrier::SCOPE_REFERENCE)?,
        ) == Some(scope.record_index),
        carrier_paired_at == carrier_at.checked_add(class_383_carrier::LEN)?,
        rigid_transform_at(bytes, carrier_at.checked_add(class_383_carrier::TRANSFORM)?)?
            == frame.transform,
    ];
    if structural_checks.iter().any(|check| !check) {
        return None;
    }
    for (ordinal, record_index) in placement_owners.into_iter().enumerate() {
        if marked_record_reference(
            bytes,
            carrier_at.checked_add(
                class_383_carrier::PLACEMENT_OWNER_REFERENCES
                    .checked_add(ordinal * ASSEMBLY_MARKED_REFERENCE_LEN)?,
            )?,
        ) != Some(record_index)
        {
            return None;
        }
    }
    let (
        leading_occurrence_guid,
        leading_identity_guid,
        occurrence_guid_offset,
        identity_guid_offset,
    ) = exact_legacy_class_383_identity_guids(bytes, leading_identity_at)?;
    for identity_at in [
        child_identity_at,
        first_face_identity_at,
        second_face_identity_at,
    ] {
        let (occurrence_guid, identity_guid, _, _) =
            exact_legacy_class_383_identity_guids(bytes, identity_at)?;
        if occurrence_guid != leading_occurrence_guid || identity_guid != leading_identity_guid {
            return None;
        }
    }
    let scope_at = usize::try_from(scope.byte_offset).ok()?;
    let locator_reference_at = scope_at.checked_add(spec.scope_operand_reference_offset)?;
    let (locator_record_index, locator_reference_offset) =
        exact_same_segment_record_reference(bytes, locator_reference_at)?;
    if locator_record_index != carrier_record_index {
        return None;
    }
    let (scope_record_index, locator_scope_reference_offset) = exact_same_segment_record_reference(
        bytes,
        carrier_at.checked_add(class_383_carrier::SCOPE_REFERENCE)?,
    )?;
    if scope_record_index != scope.record_index {
        return None;
    }
    let (_, wrapper_reference_offset) = exact_same_segment_record_reference(
        bytes,
        leading_at.checked_add(class_383_leading::IDENTITY_REFERENCE)?,
    )?;
    Some(DesignAssemblyOperandPath {
        link: DesignAssemblyOperandPathLink {
            locator_reference_offset,
            locator_record_index,
            locator_class_tag: "378".into(),
            locator_byte_offset: u64::try_from(carrier_at).ok()?,
            locator_scope_reference_offset,
            wrapper_record_index: leading_identity_record_index,
            wrapper_reference_offset,
            wrapper_class_tag: "359".into(),
            wrapper_byte_offset: u64::try_from(leading_identity_at).ok()?,
            path_reference_offset: occurrence_guid_offset,
        },
        record_index: leading_identity_record_index,
        class_tag: "386".into(),
        byte_offset: u64::try_from(leading_identity_at).ok()?,
        occurrence_guids: vec![crate::records::Located { value: leading_occurrence_guid, offset: occurrence_guid_offset }],
        identity_guids: vec![crate::records::Located { value: leading_identity_guid, offset: identity_guid_offset }],
    })
}

fn exact_legacy_class_383_record_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &str,
    frame_length: usize,
) -> Option<(usize, usize)> {
    let mut candidates = records.frames(record_index).filter(|(start, paired_at)| {
        *paired_at == start.saturating_add(frame_length)
            && exact_indexed_header_at(bytes, *start, record_index).as_deref() == Some(class_tag)
            && exact_indexed_header_at(bytes, *paired_at, record_index).as_deref() == Some("258")
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn exact_legacy_class_383_identity_guids(
    bytes: &[u8],
    start: usize,
) -> Option<(String, String, u64, u64)> {
    let first_at = start.checked_add(class_383_identity::OCCURRENCE_GUID)?;
    let second_at = start.checked_add(class_383_identity::IDENTITY_GUID)?;
    let (occurrence_guid, after_occurrence) = lp_utf16_bounded(bytes, first_at, 36..=36)?;
    let (identity_guid, after_identity) = lp_utf16_bounded(bytes, second_at, 36..=36)?;
    if !crate::bytes::is_guid_relaxed(&occurrence_guid)
        || !crate::bytes::is_guid_relaxed(&identity_guid)
        || after_occurrence != second_at
        || after_identity
            != start
                .checked_add(class_383_identity::IDENTITY_GUID)?
                .checked_add(76)?
    {
        return None;
    }
    Some((
        occurrence_guid,
        identity_guid,
        u64::try_from(first_at.checked_add(4)?).ok()?,
        u64::try_from(second_at.checked_add(4)?).ok()?,
    ))
}

struct LegacyClass412Path {
    record_index: u32,
    byte_offset: u64,
    occurrence_guid: crate::records::Located<String>,
    identity_guids: Vec<crate::records::Located<String>>,
}

fn exact_legacy_class_388_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<[DesignAssemblyOperandPath; 2]> {
    if !matches!(
        crate::design::assembly::operand_frame_variant(
            scope.frame_length,
            &scope.class_tag,
            &scope.paired_class_tag,
        ),
        Some(crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388)
    ) || scope.reference_members.len() != class_388_assemble::REFERENCE_COUNT_VALUE as usize
    {
        return None;
    }
    let scope_at = usize::try_from(scope.byte_offset).ok()?;
    let search_start = usize::try_from(scope.paired_byte_offset)
        .ok()?
        .checked_add(11)?;
    let locator_offsets = [
        class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES,
        class_388_assemble::OPERAND_PATH_LOCATOR_REFERENCES + 11,
    ];
    let paths = locator_offsets.map(|relative_offset| {
        let locator_reference_at = scope_at.checked_add(relative_offset)?;
        let (locator_record_index, locator_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_reference_at)?;
        let mut candidates = records
            .offsets(locator_record_index)
            .iter()
            .copied()
            .filter(|locator_at| *locator_at >= search_start)
            .filter_map(|locator_at| {
                exact_legacy_class_388_operand_path_envelope(
                    bytes,
                    records,
                    scope,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                )
            });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    });
    let [Some(first), Some(second)] = paths else {
        return None;
    };
    let wrapper_end = |path: &DesignAssemblyOperandPath| {
        let wrapper_at = usize::try_from(path.link.wrapper_byte_offset).ok()?;
        next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)
    };
    let first_start = usize::try_from(first.link.locator_byte_offset).ok()?;
    let first_end = wrapper_end(&first)?;
    let second_start = usize::try_from(second.link.locator_byte_offset).ok()?;
    let second_end = wrapper_end(&second)?;
    if first.link.locator_record_index == second.link.locator_record_index
        || first.link.wrapper_record_index == second.link.wrapper_record_index
        || (first_start < second_end && second_start < first_end)
    {
        return None;
    }
    Some([first, second])
}

fn exact_legacy_class_388_operand_path_envelope(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Option<DesignAssemblyOperandPath> {
    let locator_class_tag = exact_indexed_header_at(bytes, locator_at, locator_record_index)?;
    if locator_class_tag != "451"
        || next_indexed_record_offset(bytes, locator_at.checked_add(1)?)?
            != locator_at.checked_add(path_locator::LEN)?
        || bytes.get(
            locator_at.checked_add(path_locator::ZERO_RUN_10)?
                ..locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
        )? != [0; 10]
        || exact_same_segment_record_reference(
            bytes,
            locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
        )?
        .0 == 0
        || bytes.get(locator_at.checked_add(path_locator::ZERO_32)?) != Some(&0)
        || rigid_transform_at(bytes, locator_at.checked_add(path_locator::TRANSFORM)?).is_none()
        || bytes.get(locator_at.checked_add(path_locator::ZERO_161)?) != Some(&0)
    {
        return None;
    }
    let (scope_record_index, locator_scope_reference_offset) = exact_same_segment_record_reference(
        bytes,
        locator_at.checked_add(path_locator::SCOPE_BACKLINK)?,
    )?;
    if scope_record_index != scope.record_index {
        return None;
    }
    let (wrapper_record_index, wrapper_reference_offset) = exact_same_segment_record_reference(
        bytes,
        locator_at.checked_add(path_locator::WRAPPER_REFERENCE)?,
    )?;
    if wrapper_record_index == 0
        || View::u32_le_at(bytes, locator_at.checked_add(path_locator::CONSTANT_TWO)?)? != 2
        || bytes.get(
            locator_at.checked_add(path_locator::ZERO_TAIL_2)?
                ..locator_at.checked_add(path_locator::LEN)?,
        )? != [0; 2]
    {
        return None;
    }
    let locator_end = locator_at.checked_add(path_locator::LEN)?;
    let mut wrapper_candidates = records
        .offsets(wrapper_record_index)
        .iter()
        .copied()
        .filter(|wrapper_at| *wrapper_at >= locator_end)
        .filter(|wrapper_at| {
            exact_indexed_header_at(bytes, *wrapper_at, wrapper_record_index).as_deref()
                == Some("369")
        });
    let wrapper_at = wrapper_candidates.next()?;
    if wrapper_candidates.next().is_some() {
        return None;
    }
    let wrapper_class_tag = exact_indexed_header_at(bytes, wrapper_at, wrapper_record_index)?;
    if bytes.get(
        wrapper_at.checked_add(11)?
            ..wrapper_at.checked_add(class_369_wrapper_one::WRAPPER_MARKER)?,
    )? != [0; 10]
        || bytes.get(wrapper_at.checked_add(class_369_wrapper_one::WRAPPER_MARKER)?) != Some(&1)
    {
        return None;
    }
    let path_count = View::u32_le_at(
        bytes,
        wrapper_at.checked_add(class_369_wrapper_one::PATH_COUNT)?,
    )?;
    let (wrapper_length, path_reference_offset) = match path_count {
        value if value == class_369_wrapper_one::PATH_COUNT_VALUE => (
            class_369_wrapper_one::LEN,
            class_369_wrapper_one::PATH_REFERENCE,
        ),
        value if value == class_369_wrapper_two::PATH_COUNT_VALUE => (
            class_369_wrapper_two::LEN,
            class_369_wrapper_two::PATH_REFERENCES,
        ),
        _ => return None,
    };
    let wrapper_end = wrapper_at.checked_add(wrapper_length)?;
    if next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)? != wrapper_end
        || wrapper_record_index
            != locator_record_index
                .checked_add(path_count)?
                .checked_add(1)?
    {
        return None;
    }
    let mut path_at = next_indexed_record_offset(bytes, locator_end)?;
    let mut path_records = Vec::with_capacity(path_count as usize);
    let mut final_path_reference_offset = None;
    for ordinal in 0..path_count {
        let path_record_index = locator_record_index.checked_add(ordinal)?.checked_add(1)?;
        if exact_indexed_header_at(bytes, path_at, path_record_index).as_deref() != Some("412") {
            return None;
        }
        let path_end = next_indexed_record_offset(bytes, path_at.checked_add(1)?)?;
        let path = exact_legacy_class_412_path(bytes, path_at, path_record_index, path_end)?;
        path_records.push(path);
        let (referenced_path_record_index, reference_offset) = exact_same_segment_record_reference(
            bytes,
            wrapper_at
                .checked_add(path_reference_offset)?
                .checked_add(usize::try_from(ordinal).ok()?.checked_mul(11)?)?,
        )?;
        if referenced_path_record_index != path_record_index {
            return None;
        }
        if ordinal + 1 == path_count {
            if path_end != wrapper_at {
                return None;
            }
            final_path_reference_offset = Some(reference_offset);
        } else {
            path_at = path_end;
        }
    }
    let final_path = path_records.pop()?;
    let occurrence_guids = path_records
        .iter()
        .map(|path| path.occurrence_guid.clone())
        .chain(std::iter::once(final_path.occurrence_guid.clone()))
        .collect::<Vec<_>>();
    Some(DesignAssemblyOperandPath {
        link: DesignAssemblyOperandPathLink {
            locator_reference_offset,
            locator_record_index,
            locator_class_tag,
            locator_byte_offset: u64::try_from(locator_at).ok()?,
            locator_scope_reference_offset,
            wrapper_record_index,
            wrapper_reference_offset,
            wrapper_class_tag,
            wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
            path_reference_offset: final_path_reference_offset?,
        },
        record_index: final_path.record_index,
        class_tag: "412".into(),
        byte_offset: final_path.byte_offset,
        occurrence_guids,
        identity_guids: final_path.identity_guids,
    })
}

fn exact_legacy_class_412_path(
    bytes: &[u8],
    start: usize,
    record_index: u32,
    end: usize,
) -> Option<LegacyClass412Path> {
    if exact_indexed_header_at(bytes, start, record_index)?.as_str() != "412"
        || end != start.checked_add(class_412_path::LEN)?
        || bytes.get(start.checked_add(11)?..start.checked_add(class_412_path::PATH_MARKER)?)?
            != [0; 10]
        || bytes.get(start.checked_add(class_412_path::PATH_MARKER)?)
            != Some(&class_412_path::PATH_MARKER_VALUE)
        || bytes.get(
            start.checked_add(class_412_path::PATH_MARKER + 1)?
                ..start.checked_add(class_412_path::OCCURRENCE_GUID)?,
        )? != [0; 3]
        || View::u64_le_at(
            bytes,
            start.checked_add(class_412_path::IDENTITY_SEPARATOR)?,
        )? != class_412_path::IDENTITY_SEPARATOR_VALUE
        || View::u32_le_at(bytes, start.checked_add(class_412_path::PATH_TAIL_COUNT)?)?
            != class_412_path::PATH_TAIL_COUNT_VALUE
        || bytes.get(start.checked_add(class_412_path::PATH_TAIL_COUNT + 4)?..end)? != [0; 8]
    {
        return None;
    }
    let (occurrence_guid, occurrence_end) = lp_utf16_bounded(
        bytes,
        start.checked_add(class_412_path::OCCURRENCE_GUID)?,
        36..=36,
    )?;
    if !is_guid_relaxed(&occurrence_guid)
        || occurrence_end != start.checked_add(class_412_path::FIRST_IDENTITY_GUID)?
    {
        return None;
    }
    let identity_offsets = [
        class_412_path::FIRST_IDENTITY_GUID,
        class_412_path::SECOND_IDENTITY_GUID,
        class_412_path::THIRD_IDENTITY_GUID,
        class_412_path::FOURTH_IDENTITY_GUID,
    ];
    let mut identity_guids = Vec::with_capacity(identity_offsets.len());
    for (ordinal, relative_offset) in identity_offsets.iter().copied().enumerate() {
        let identity_at = start.checked_add(relative_offset)?;
        let (identity_guid, identity_end) = lp_utf16_bounded(bytes, identity_at, 36..=36)?;
        if !is_guid_relaxed(&identity_guid) {
            return None;
        }
        let expected_end = match ordinal {
            0 => class_412_path::SECOND_IDENTITY_GUID,
            1 => class_412_path::IDENTITY_SEPARATOR,
            2 => class_412_path::FOURTH_IDENTITY_GUID,
            3 => class_412_path::PATH_TAIL_COUNT,
            _ => return None,
        };
        if identity_end != start.checked_add(expected_end)? {
            return None;
        }
        identity_guids.push(crate::records::Located { value: identity_guid, offset: u64::try_from(identity_at.checked_add(4)?).ok()? });
    }
    Some(LegacyClass412Path {
        record_index,
        byte_offset: u64::try_from(start).ok()?,
        occurrence_guid: crate::records::Located { value: occurrence_guid, offset: u64::try_from(
            start.checked_add(class_412_path::OCCURRENCE_GUID + 4)?,
        ).ok()? },
        identity_guids,
    })
}

fn exact_as_built_operand_frames(
    bytes: &[u8],
    paths: &[DesignAssemblyOperandPath; 2],
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let frames = paths.each_ref().map(|path| {
        let locator_at = usize::try_from(path.link.locator_byte_offset).ok()?;
        let reference_at = locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?;
        let transform_at = locator_at.checked_add(path_locator::TRANSFORM)?;
        Some(DesignAssemblyOperandFrame {
            reference_record_index: marked_record_reference(bytes, reference_at)?,
            reference_offset: u64::try_from(reference_at.checked_add(1)?).ok()?,
            transform: rigid_transform_at(bytes, transform_at)?,
            transform_offset: u64::try_from(transform_at).ok()?,
        })
    });
    let [Some(first), Some(second)] = frames else {
        return None;
    };
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

fn exact_assembly_operand_paths(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<[DesignAssemblyOperandPath; 2]> {
    let scope_at = usize::try_from(scope.byte_offset).ok()?;
    let search_start = usize::try_from(scope.paired_byte_offset)
        .ok()?
        .checked_add(11)?;
    let locator_offsets = crate::design::assembly::operand_path_locator_offsets(
        scope.frame_length,
        &scope.class_tag,
        &scope.paired_class_tag,
    )?;
    let count_at = scope_at
        .checked_add(locator_offsets[0].checked_sub(path_locator_run::FIRST_LOCATOR_REFERENCE)?)?;
    if View::u32_le_at(bytes, count_at)? != 2 {
        return None;
    }
    let paths = locator_offsets.map(|relative_offset| {
        let locator_reference_at = scope_at.checked_add(relative_offset)?;
        let (locator_record_index, locator_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_reference_at)?;
        let mut candidates = records
            .offsets(locator_record_index)
            .iter()
            .copied()
            .filter(|locator_at| *locator_at >= search_start)
            .filter_map(|locator_at| {
                exact_assembly_operand_path_envelope(
                    bytes,
                    scope,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                )
            });
        let candidate = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(candidate)
    });
    let [Some(first), Some(second)] = paths else {
        return None;
    };
    let first_start = usize::try_from(first.link.locator_byte_offset).ok()?;
    let first_end = next_indexed_record_offset(
        bytes,
        usize::try_from(first.link.wrapper_byte_offset)
            .ok()?
            .checked_add(1)?,
    )?;
    let second_start = usize::try_from(second.link.locator_byte_offset).ok()?;
    let second_end = next_indexed_record_offset(
        bytes,
        usize::try_from(second.link.wrapper_byte_offset)
            .ok()?
            .checked_add(1)?,
    )?;
    if first.link.locator_record_index == second.link.locator_record_index
        || (first_start < second_end && second_start < first_end)
    {
        return None;
    }
    Some([first, second])
}

fn exact_assembly_operand_path_envelope(
    bytes: &[u8],
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Option<DesignAssemblyOperandPath> {
    let locator_class_tag = exact_indexed_header_at(bytes, locator_at, locator_record_index)?;
    let variable_reference = crate::design::assembly::variable_reference_assembly_generation(
        &scope.class_tag,
        &scope.paired_class_tag,
    );
    let (locator_length, scope_backlink, wrapper_reference, constant_two, zero_tail) =
        if variable_reference {
            if locator_class_tag != "390"
                || bytes
                    .get(
                        locator_at.checked_add(variable_path_locator::TRANSFORM + 16 * 8)?
                            ..locator_at.checked_add(variable_path_locator::SCOPE_BACKLINK)?,
                    )?
                    .iter()
                    .any(|byte| *byte != 0)
                || rigid_transform_at(
                    bytes,
                    locator_at.checked_add(variable_path_locator::TRANSFORM)?,
                )
                .is_none()
            {
                return None;
            }
            (
                variable_path_locator::LEN,
                variable_path_locator::SCOPE_BACKLINK,
                variable_path_locator::WRAPPER_REFERENCE,
                variable_path_locator::CONSTANT_TWO,
                variable_path_locator::ZERO_TAIL,
            )
        } else {
            if bytes.get(
                locator_at.checked_add(path_locator::ZERO_RUN_10)?
                    ..locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
            )? != [0; 10]
                || exact_same_segment_record_reference(
                    bytes,
                    locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
                )?
                .0 == 0
                || bytes.get(locator_at.checked_add(path_locator::ZERO_32)?) != Some(&0)
                || rigid_transform_at(bytes, locator_at.checked_add(path_locator::TRANSFORM)?)
                    .is_none()
                || bytes.get(locator_at.checked_add(path_locator::ZERO_161)?) != Some(&0)
            {
                return None;
            }
            (
                path_locator::LEN,
                path_locator::SCOPE_BACKLINK,
                path_locator::WRAPPER_REFERENCE,
                path_locator::CONSTANT_TWO,
                path_locator::ZERO_TAIL_2,
            )
        };
    let (scope_record_index, locator_scope_reference_offset) =
        exact_same_segment_record_reference(bytes, locator_at.checked_add(scope_backlink)?)?;
    let (wrapper_record_index, wrapper_reference_offset) =
        exact_same_segment_record_reference(bytes, locator_at.checked_add(wrapper_reference)?)?;
    let path_record_index = locator_record_index.checked_add(1)?;
    if scope_record_index != scope.record_index
        || if variable_reference {
            !(locator_record_index.checked_add(2)?..=locator_record_index.checked_add(65)?)
                .contains(&wrapper_record_index)
        } else {
            wrapper_record_index != locator_record_index.checked_add(2)?
        }
        || View::u32_le_at(bytes, locator_at.checked_add(constant_two)?)? != 2
        || bytes.get(locator_at.checked_add(zero_tail)?..locator_at.checked_add(locator_length)?)?
            != [0; 2]
    {
        return None;
    }
    let path_at = locator_at.checked_add(locator_length)?;
    if next_indexed_record_offset(bytes, locator_at.checked_add(1)?)? != path_at {
        return None;
    }
    let mut path_spans = Vec::new();
    let mut record_index = path_record_index;
    let mut record_at = path_at;
    let wrapper_at = loop {
        if record_index == wrapper_record_index {
            break record_at;
        }
        let next = next_indexed_record_offset(bytes, record_at.checked_add(1)?)?;
        path_spans.push((record_index, record_at, next));
        record_index = record_index.checked_add(1)?;
        record_at = next;
    };
    let wrapper_class_tag = exact_indexed_header_at(bytes, wrapper_at, wrapper_record_index)?;
    let wrapper_end = next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)?;
    let expected_wrapper_length = if variable_reference {
        path_wrapper::LEN.checked_add(path_spans.len().checked_sub(1)?.checked_mul(11)?)?
    } else {
        path_wrapper::LEN
    };
    if variable_reference && wrapper_class_tag != "397"
        || bytes.get(
            wrapper_at.checked_add(path_wrapper::ZERO_RUN_10)?
                ..wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?,
        )? != [0; 10]
        || bytes.get(wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?) != Some(&1)
        || View::u32_le_at(
            bytes,
            wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_WORD)?,
        )? != if variable_reference {
            u32::try_from(path_spans.len()).ok()?
        } else {
            1
        }
        || wrapper_end != wrapper_at.checked_add(expected_wrapper_length)?
    {
        return None;
    }
    let (referenced_path_record_index, path_reference_offset) =
        exact_same_segment_record_reference(
            bytes,
            wrapper_at.checked_add(path_wrapper::PATH_REFERENCE)?,
        )?;
    if referenced_path_record_index != path_record_index {
        return None;
    }
    if variable_reference
        && path_spans
            .iter()
            .skip(1)
            .enumerate()
            .any(|(ordinal, (record_index, _, _))| {
                exact_same_segment_record_reference(
                    bytes,
                    wrapper_at + path_wrapper::LEN + ordinal * 11,
                )
                .map(|reference| reference.0)
                    != Some(*record_index)
            })
    {
        return None;
    }
    let link = DesignAssemblyOperandPathLink {
        locator_reference_offset,
        locator_record_index,
        locator_class_tag,
        locator_byte_offset: u64::try_from(locator_at).ok()?,
        locator_scope_reference_offset,
        wrapper_record_index,
        wrapper_reference_offset,
        wrapper_class_tag,
        wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
        path_reference_offset,
    };
    let mut paths = path_spans.into_iter().map(|(record_index, start, limit)| {
        exact_assembly_operand_path(bytes, start, record_index, limit, link.clone())
    });
    let mut path = paths.next()??;
    if variable_reference && path.class_tag != "330" {
        return None;
    }
    for continuation in paths {
        let continuation = continuation?;
        if !variable_reference || continuation.class_tag != "330" {
            return None;
        }
        path.occurrence_guids.extend(continuation.occurrence_guids);
        path.identity_guids.extend(continuation.identity_guids);
    }
    Some(path)
}

fn exact_assembly_operand_path(
    bytes: &[u8],
    start: usize,
    record_index: u32,
    limit: usize,
    link: DesignAssemblyOperandPathLink,
) -> Option<DesignAssemblyOperandPath> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 1..=8, u8::is_ascii_digit)?;
    if View::u64_le_at(bytes, after_tag)? != u64::from(record_index) {
        return None;
    }
    let mut occurrence_guids = Vec::new();
    let mut identity_guids = Vec::new();
    match class_tag.as_str() {
        "294" | "299" | "307" => {
            let end = next_indexed_record_offset(bytes, start + 1)?;
            if end != limit
                || bytes.get(after_tag + 8..after_tag + 14)? != [0; 6]
                || bytes.get(after_tag + 14) != Some(&1)
                || bytes.get(after_tag + 15..after_tag + 18)? != [0; 3]
            {
                return None;
            }
            let mut position = after_tag + 18;
            let (occurrence, after_occurrence) =
                lp_utf16_bounded(bytes.get(..end)?, position, 36..=36)?;
            if !crate::bytes::is_guid_relaxed(&occurrence) {
                return None;
            }
            occurrence_guids.push(crate::records::Located { value: occurrence, offset: u64::try_from(position + 4).ok()? });
            position = after_occurrence;
            for _ in 0..2 {
                let (guid, after_guid) = lp_utf16_bounded(bytes.get(..end)?, position, 36..=36)?;
                if !crate::bytes::is_guid_relaxed(&guid) {
                    return None;
                }
                identity_guids.push(crate::records::Located { value: guid, offset: u64::try_from(position + 4).ok()? });
                position = after_guid;
            }
            if View::u64_le_at(bytes, position)? != 2 {
                return None;
            }
            position += 8;
            for _ in 0..2 {
                let (guid, after_guid) = lp_utf16_bounded(bytes.get(..end)?, position, 36..=36)?;
                if !crate::bytes::is_guid_relaxed(&guid) {
                    return None;
                }
                identity_guids.push(crate::records::Located { value: guid, offset: u64::try_from(position + 4).ok()? });
                position = after_guid;
            }
            if View::u32_le_at(bytes, position)? != 2
                || !bytes.get(position + 4..end)?.iter().all(|byte| *byte == 0)
            {
                return None;
            }
        }
        "329" | "330" | "386" | "390" => {
            if bytes.get(after_tag + 8..after_tag + 14)? != [0; 6] {
                return None;
            }
            let count = usize::try_from(View::u32_le_at(bytes, after_tag + 14)?).ok()?;
            if !(1..=64).contains(&count) {
                return None;
            }
            let mut position = after_tag + 18;
            for _ in 0..count {
                let (guid, after_guid) = lp_utf16_bounded(bytes.get(..limit)?, position, 36..=36)?;
                if !crate::bytes::is_guid_relaxed(&guid) {
                    return None;
                }
                occurrence_guids.push(crate::records::Located { value: guid, offset: u64::try_from(position + 4).ok()? });
                position = after_guid;
            }
            if position == limit {
                if !matches!(class_tag.as_str(), "329" | "330") {
                    return None;
                }
            } else {
                for _ in 0..2 {
                    let (guid, after_guid) =
                        lp_utf16_bounded(bytes.get(..limit)?, position, 36..=36)?;
                    if !crate::bytes::is_guid_relaxed(&guid) {
                        return None;
                    }
                    identity_guids.push(crate::records::Located { value: guid, offset: u64::try_from(position + 4).ok()? });
                    position = after_guid;
                }
                if View::u64_le_at(bytes, position)? != 2 {
                    return None;
                }
                position += 8;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        lp_utf16_bounded(bytes.get(..limit)?, position, 36..=36)?;
                    if !crate::bytes::is_guid_relaxed(&guid) {
                        return None;
                    }
                    identity_guids.push(crate::records::Located { value: guid, offset: u64::try_from(position + 4).ok()? });
                    position = after_guid;
                }
                if View::u32_le_at(bytes, position)? != 2
                    || !bytes
                        .get(position + 4..limit)?
                        .iter()
                        .all(|byte| *byte == 0)
                {
                    return None;
                }
            }
        }
        _ => return None,
    }
    Some(DesignAssemblyOperandPath {
        link,
        record_index,
        class_tag,
        byte_offset: u64::try_from(start).ok()?,
        occurrence_guids,
        identity_guids,
    })
}

pub(crate) fn exact_rectangular_pattern_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignRectangularPatternConstruction> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::RectangularPattern) {
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
        || !scope.reference_members.values().skip(1).take(4).eq(construction.owner_record_indices.iter())
    {
        return None;
    }
    let mut record_indices = Vec::with_capacity(count);
    record_indices.push(*scope.reference_members.values().next()?);
    record_indices.extend(scope.reference_members.values_in(6..count.checked_add(5)?)?.copied());
    let reference_starts = scope
        .reference_members
        .values()
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
            if (distance - extent.abs()).abs() > EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8 {
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
                                .all(|(value, total)| {
                                    (*value - total * fraction).abs()
                                        <= EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8
                                })
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
    runs.sort_by(|a, b| {
        a.iter()
            .map(|(_, offset)| *offset)
            .cmp(b.iter().map(|(_, offset)| *offset))
    });
    runs.dedup_by(|left, right| left == right);
    let [run] = runs.as_slice() else {
        return None;
    };
    Some(DesignRectangularPatternInstances::Bodies(record_indices.into_iter().zip(run).map(|(record_index, (value, offset))| crate::records::DesignPatternInstance {
        record_index,
        transform: crate::records::Located { value: *value, offset: *offset },
    }).collect()))
}

type TransformCandidate = ([[f64; 4]; 4], u64);

fn exact_rigid_transform_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Option<Vec<TransformCandidate>> {
    /// The single byte image of `1.0_f64`, the last lane of a rigid
    /// transform's fixed `0 0 0 1` bottom row.
    const ONE_F64_LE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xF0, 0x3F];
    let last_exclusive = end.checked_sub(127)?;
    if start >= last_exclusive || end > bytes.len() {
        // An exhaustive scan over this range finds nothing or aborts on its
        // first out-of-bounds sixteen-lane read.
        return None;
    }
    let mut candidates = Vec::new();
    // A valid transform carries exactly `1.0` at lane fifteen and a zero of
    // either sign at lanes twelve to fourteen. Locate the fixed `1.0` image
    // (it cannot overlap itself, so every occurrence surfaces) and read the
    // full sixteen-lane frame only at surviving offsets.
    for hit in memchr::memmem::find_iter(&bytes[start + 120..end], &ONE_F64_LE) {
        let offset = start + hit;
        let zero_lanes_valid = (0..3).all(|lane| {
            let at = offset + 96 + lane * 8;
            bytes[at..at + 7].iter().all(|byte| *byte == 0)
                && (bytes[at + 7] == 0 || bytes[at + 7] == 0x80)
        });
        if !zero_lanes_valid {
            continue;
        }
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
    (0..3).all(|row| {
        (0..3).all(|column| {
            (left[row][column] - right[row][column]).abs() <= EPS_SCOPES_SAME_TRANSFORM_BASIS_E10
        })
    })
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
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::CircularPattern) {
        return None;
    }
    let mut axis_candidates = Vec::new();
    for (record_index, selection_record_index) in scope.reference_members.values()
        .zip(scope.reference_members.values().skip(1)) {
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
    for record_index in scope.reference_members.values() {
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
    let (axis, record_index, selection_record_index) =
        select_circular_pattern_axis(&axis_candidates)?;
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
        count_candidates.extend(scope.reference_members.values().filter_map(|record_index| {
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
        angle_candidates.extend(scope.reference_members.values().filter_map(|record_index| {
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

pub(crate) type CircularPatternAxisCandidate =
    (crate::records::DesignCircularPatternAxis, u32, u32);

/// Select one circular-pattern axis, preferring the explicit solved carrier.
pub(crate) fn select_circular_pattern_axis(
    candidates: &[CircularPatternAxisCandidate],
) -> Option<&CircularPatternAxisCandidate> {
    let inline = candidates
        .iter()
        .filter(|(axis, _, _)| {
            matches!(
                axis,
                crate::records::DesignCircularPatternAxis::Inline { .. }
            )
        })
        .collect::<Vec<_>>();
    match inline.as_slice() {
        [candidate] => Some(*candidate),
        [] => match candidates {
            [candidate] => Some(candidate),
            _ => None,
        },
        _ => None,
    }
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
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
    {
        return None;
    }
    let (identity_offsets, selection_at, second_count_at, second_identity_at, scope_at, tail_at) =
        match (
            paired_at.checked_sub(start),
            View::u32_le_at(bytes, start + 21),
        ) {
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
            || View::u32_le_at(bytes, start + 36) != Some(1)))
        || View::u32_le_at(bytes, second_count_at) != Some(1)
        || marked_record_reference(bytes, second_identity_at).is_none()
        || bytes.get(second_identity_at + 5..second_identity_at + 11) != Some(&[0; 6])
        || marked_record_reference(bytes, scope_at) != Some(scope.record_index)
        || bytes.get(scope_at + 5..scope_at + 11) != Some(&[0; 6])
    {
        return None;
    }
    let selection_record_index = marked_record_reference(bytes, selection_at)?;
    if !scope.reference_members.values().any(|value| value == &selection_record_index)
        || bytes.get(selection_at + 5..selection_at + 11) != Some(&[0; 6])
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, tail_at)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, tail_at + 4)?.is_finite()
        || View::u32_le_at(bytes, tail_at + 12) != Some(opaque_index)
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
        || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let wrappers = identity_offsets.iter().map(|offset| {
        let record_index = View::u32_le_at(bytes, *offset)?;
        let (identity, identity_offset) = exact_pattern_identity_wrapper(bytes, records, record_index)?;
        Some((identity, crate::records::DesignPatternAxisWrapper { record_index, identity_offset }))
    }).collect::<Option<Vec<_>>>()?;
    let persistent_identity = wrappers.first()?.0;
    if wrappers.iter().any(|(identity, _)| *identity != persistent_identity) {
        return None;
    }
    Some((
        DesignCircularPatternAxis::HistoricalEdge {
            wrappers: wrappers.into_iter().map(|(_, wrapper)| wrapper).collect(),
            persistent_identity,
            resolved: None,
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
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || View::u64_le_at(bytes, start + 21)? == 0
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 29, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !crate::bytes::is_guid_relaxed(&asset_id)
        || !crate::bytes::is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id) != Some(2)
        || bytes.get(after_context_id + 4..after_context_id + 8) != Some(&[0; 4])
        || marked_record_reference(bytes, after_context_id + 8) != record_index.checked_add(1)
        || bytes.get(after_context_id + 13..after_context_id + 19) != Some(&[0; 6])
    {
        return None;
    }
    let nested_one_at = next_indexed_record_offset(bytes, after_context_id + 19)?;
    let (_, nested_one_tag) = lp_ascii_filtered(bytes, nested_one_at, 3..=3, u8::is_ascii_digit)?;
    if View::u32_le_at(bytes, nested_one_tag) != record_index.checked_add(1)
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
    if View::u32_le_at(bytes, identity_tag) != record_index.checked_add(2)
        || bytes.get(identity_at + 11..identity_at + 21) != Some(&[0; 10])
        || identity_at.checked_add(29) != Some(next_at)
        || View::u32_le_at(bytes, next_tag) != record_index.checked_add(3)
    {
        return None;
    }
    Some((
        View::u64_le_at(bytes, identity_at + 21)?,
        u64::try_from(identity_at + 21).ok()?,
    ))
}

/// Parse the class-441 Mirror count owner carried outside the ordinary owner
/// arena.
///
/// The scope's fourth reference names a class-426 frame paired with class 267.
/// Its exact compact scalar envelope carries count two and has no decoded
/// Design-parameter backlink.
pub(super) fn exact_legacy_mirror_scope_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<(u32, u64)> {
    if (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) != ("441", "267") {
        return None;
    }
    let count_record_index = *scope.reference_members.values().nth(3)?;
    let [start, paired] = records.offsets(count_record_index) else {
        return None;
    };
    if paired.checked_sub(*start)? != mirror_441_count::LEN {
        return None;
    }
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered(bytes, *paired, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag != "267"
        || paired_after_tag != paired.checked_add(7)?
        || View::u32_le_at(bytes, paired_after_tag) != Some(count_record_index)
    {
        return None;
    }
    let frame = bytes.get(*start..*paired)?;
    let owner = crate::design::decode::parameters::parse_parameter_owner(frame)?;
    if owner.class_tag != "426"
        || owner.record_index != count_record_index
        || owner.scope_record_index != scope.record_index
        || owner.local_ordinal != mirror_441_count::LOCAL_ORDINAL_VALUE
        || owner.owned_ordinal != mirror_441_count::OWNED_ORDINAL_VALUE
        || owner.evaluated_value != f64::from(mirror_441_count::COUNT_VALUE)
        || owner.frame_length != u64::try_from(mirror_441_count::LEN).ok()?
        || owner.parameter_record_index != count_record_index.checked_add(2)?
        || owner.companion_record_index != count_record_index.checked_add(1)?
        || owner.evaluated_value_offset != u64::try_from(mirror_441_count::COUNT).ok()?
    {
        return None;
    }
    Some((
        count_record_index,
        u64::try_from(*start)
            .ok()?
            .checked_add(owner.evaluated_value_offset)?,
    ))
}

/// Parse a legacy Mirror scalar lane.
///
/// These forms have no ordinal-one parameter owner. Their positive stitch
/// tolerance is carried after the preceding-history field, with two marked
/// references naming the adjacent legacy records. The class pair selects the
/// generation-specific scalar marker; the remaining tail offsets are shared.
pub(super) fn exact_legacy_mirror_scope_tolerance(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(f64, u64, DesignMirrorScopeTolerance)> {
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
    let kind_code_units = scope.kind_name().encode_utf16().count();
    let kind_end = usize::try_from(scope.kind_offset)
        .ok()?
        .checked_add(kind_code_units.checked_mul(2)?)?;
    let previous = usize::try_from(scope.previous_history_state_id_offset?).ok()?;
    if previous != kind_end.checked_add(previous_state)? {
        return None;
    }
    let paired = usize::try_from(scope.paired_byte_offset).ok()?;
    if paired != kind_end.checked_add(tail_length)?
        || paired.checked_sub(usize::try_from(scope.byte_offset).ok()?)?
            != usize::try_from(scope.frame_length).ok()?
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
    let value = View::f64_le_at(bytes, value_offset)?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
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
        u64::try_from(value_offset).ok()?,
        DesignMirrorScopeTolerance {
            marker: crate::records::DesignMirrorToleranceMarker::try_from((marker, repeated_marker_offset.map(u64::try_from).transpose().ok()?)).ok()?,
            marker_offset: u64::try_from(marker_offset).ok()?,
            first_reference,
            first_reference_offset: u64::try_from(first_reference_offset).ok()?,
            second_reference,
            second_reference_offset: u64::try_from(second_reference_offset).ok()?,
        },
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
    recipes: &[ConstructionRecipe],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();
    for index in 0..scopes.len() {
        if design_feature_family(&scopes[index].kind()) != Some(DesignFeatureFamily::Mirror) {
            continue;
        }
        let Some(stream) = native_stream(&scopes[index].id).map(str::to_owned) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope_record_index = scopes[index].record_index;
        let scope_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream.as_str())
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
        let [crate::records::Located { value: plane_member, .. }] = plane_group.members.as_slice() else {
            continue;
        };
        let Some(plane_header) = headers.get(&(stream.as_str(), *plane_member)) else {
            continue;
        };
        let work_plane = compact_feature_reference(bytes, plane_header).and_then(
            |(plane_reference, plane_reference_offset)| {
                plane_reference
                    .checked_add(1)
                    .filter(|record_index| {
                        scopes.iter().any(|scope| {
                            native_stream(&scope.id) == Some(stream.as_str())
                                && scope.record_index == *record_index
                                && scope.kind() == crate::records::DesignFeatureKind::WorkPlane
                                && scope.work_plane_frame().is_some()
                        })
                    })
                    .map(|record_index| (record_index, plane_reference_offset))
            },
        );
        let face_recipe = {
            let records = record_offset_index
                .entry(stream.clone())
                .or_insert_with(|| IndexedRecordOffsets::build(bytes));
            parse_face_operand(
                bytes,
                records,
                &scopes[index],
                plane_group.scope_reference_ordinal,
                Some((plane_group.record_index, 0)),
                None,
                plane_header,
                recipes,
            )
            .is_some()
        };
        let (plane_scope_record_index, plane_selection_record_index) =
            if let Some((plane_scope_record_index, plane_reference_offset)) = work_plane {
                (
                    Some(crate::records::Located { value: plane_scope_record_index, offset: plane_reference_offset }),
                    None,
                )
            } else if crate::design::decode::operands::parse_entity_selection_operand(
                bytes,
                plane_group,
                0,
                plane_header,
            )
            .is_some()
                || face_recipe
            {
                (None, Some(*plane_member))
            } else {
                continue;
            };
        let seed_feature = match seed_group.members.as_slice() {
            _ if seed_group.role != 0x0000_0008_0000_0000 => None,
            [crate::records::Located { value: member, .. }] => headers
                .get(&(stream.as_str(), *member))
                .and_then(|header| compact_feature_reference(bytes, header))
                .filter(|(record_index, _)| {
                    scopes.iter().any(|scope| {
                        native_stream(&scope.id) == Some(stream.as_str())
                            && scope.record_index == *record_index
                    })
                }),
            _ => None,
        };
        let scope_owners = owners
            .iter()
            .filter(|owner| {
                native_stream(&owner.id) == Some(stream.as_str())
                    && owner.scope_record_index == scope_record_index
            })
            .collect::<Vec<_>>();
        let records = record_offset_index
            .entry(stream.clone())
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        let count = scope_owners
            .iter()
            .copied()
            .filter(|owner| {
                owner.local_ordinal == 0
                    && owner.evaluated_value == 2.0
                    && owner.evaluated_value.is_finite()
            })
            .collect::<Vec<_>>();
        let inline_count = exact_legacy_mirror_scope_count(bytes, records, &scopes[index]);
        let inline_tolerance = exact_legacy_mirror_scope_tolerance(bytes, &scopes[index]);
        let tolerance = scope_owners
            .iter()
            .copied()
            .filter(|owner| {
                owner.local_ordinal == 1
                    && owner.evaluated_value.is_finite()
                    && owner.evaluated_value > 0.0
            })
            .collect::<Vec<_>>();
        let (count, tolerance_source) = (
            match (count.as_slice(), inline_count) {
                ([count], None) => Some((
                    count.record_index,
                    count.evaluated_value_offset,
                )),
                ([], Some(count)) => Some(count),
                _ => None,
            },
            match (tolerance.as_slice(), inline_tolerance) {
                ([tolerance], None) => Some((
                    tolerance.evaluated_value,
                    tolerance.evaluated_value_offset,
                    crate::records::DesignMirrorToleranceSource::Owner { record_index: tolerance.record_index },
                )),
                ([], Some((value, value_offset, scope_tail))) => {
                    Some((value, value_offset, crate::records::DesignMirrorToleranceSource::Scope(scope_tail)))
                }
                _ => None,
            },
        );
        let Some((count_record_index, count_offset)) = count else {
            continue;
        };
        let Some((
            stitch_tolerance,
            stitch_tolerance_offset,
            tolerance_source,
        )) = tolerance_source
        else {
            continue;
        };
        {
            let construction = Some(DesignMirrorConstruction {
                count_record_index,
                count_offset,
                stitch_tolerance,
                stitch_tolerance_offset,
                tolerance_source,
                seed_group_record_index: seed_group.record_index,
                plane_group_record_index: plane_group.record_index,
                seed_feature_scope_record_index: seed_feature.map(|(value, offset)| crate::records::Located { value, offset }),
                plane_scope_record_index,
                plane_selection_record_index,
                plane: None,
            });
            if let crate::records::DesignScopePayload::Mirror(slot)
            | crate::records::DesignScopePayload::SymetrieMiroir(slot) =
                &mut scopes[index].payload
            {
                *slot = construction;
            }
        }
    }
    Ok(())
}

fn compact_feature_reference(bytes: &[u8], header: &DesignRecordHeader) -> Option<(u32, u64)> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || View::u32_le_at(bytes, start + 22)? != header.record_index.checked_add(3)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || View::u32_le_at(bytes, start + 32)? != 1
    {
        return None;
    }
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !crate::bytes::is_guid_relaxed(&asset_id)
        || !crate::bytes::is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
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
        if View::u32_le_at(bytes, after_tag)? != expected {
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
        View::u32_le_at(bytes, identity_at + 21)?,
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
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || paired_at.checked_sub(start) != Some(195)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, start + 21) != Some(8)
        || bytes.get(start + 73..start + 89) != Some(&[0; 16])
        || View::u32_le_at(bytes, start + 89)? == 0
        || View::u32_le_at(bytes, start + 93) != Some(1)
        || marked_record_reference(bytes, start + 97) != Some(selection_record_index)
        || bytes.get(start + 102..start + 108) != Some(&[0; 6])
        || bytes.get(start + 108..start + 110) != Some(&[0; 2])
        || View::u32_le_at(bytes, start + 110) != Some(1)
        || marked_record_reference(bytes, start + 114).is_none()
        || bytes.get(start + 119..start + 125) != Some(&[0; 6])
        || View::u64_le_at(bytes, start + 125) != Some(0x0000_0004_0000_0000)
        || bytes.get(start + 133..start + 143) != Some(&[0; 10])
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, start + 143)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, start + 147)?.is_finite()
        || View::u32_le_at(bytes, start + 155) != Some(opaque_index)
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
        || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let origin: [f64; 3] = f64s_at(bytes, start + 25, 3)?.try_into().ok()?;
    let displacement: [f64; 3] = f64s_at(bytes, start + 49, 3)?.try_into().ok()?;
    let displacement_length = displacement[0]
        .hypot(displacement[1])
        .hypot(displacement[2]);
    if origin.iter().any(|coordinate| !coordinate.is_finite())
        || displacement
            .iter()
            .any(|coordinate| !coordinate.is_finite())
        || !displacement_length.is_finite()
        || displacement_length <= f64::EPSILON
    {
        return None;
    }
    let direction =
        if (displacement_length - 1.0).abs() <= EPS_SCOPES_EXACT_CIRCULAR_PATTERN_AXIS_E12 {
            displacement
        } else {
            displacement.map(|component| component / displacement_length)
        };
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
                || View::u32_le_at(bytes, after_tag) != Some(record_index)
                || paired_at.checked_sub(start) != Some(99)
                || bytes.get(start + 11..start + 19) != Some(&[0; 8])
                || bytes.get(start + 19) != Some(&1)
                || View::u32_le_at(bytes, start + 20) != Some(1)
                || marked_record_reference(bytes, start + 24) != Some(scope_record_index)
                || bytes.get(start + 29..start + 40) != Some(&[0; 11])
                || marked_record_reference(bytes, start + 44) != record_index.checked_add(2)
                || bytes.get(start + 49..start + 55) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 55)? == 0
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
            let count = View::u32_le_at(bytes, start + 40)?;
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
    if scope.kind() != crate::records::DesignFeatureKind::CopyPasteBodies
        || scope.reference_members.len() < 2
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let body_group_record_index = marked_record_reference(bytes, start + 29)?;
    let relation_record_index = marked_record_reference(bytes, start + 40)?;
    if *scope.reference_members.values().next()? != body_group_record_index {
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
    let body_group_count = usize::try_from(View::u32_le_at(bytes, body_group_count_at)?).ok()?;
    if body_group_count != scope.reference_members.len().checked_sub(1)? {
        return None;
    }
    let mut operands = Vec::with_capacity(body_group_count);
    let mut body_group_cursor = body_group_count_at.checked_add(4)?;
    for expected in scope.reference_members.values().skip(1) {
        let actual = marked_record_reference(bytes, body_group_cursor)?;
        if actual != *expected {
            return None;
        }
        operands.push(crate::records::Located { value: actual, offset: u64::try_from(body_group_cursor + 1).ok()? });
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
    let reference_count = usize::try_from(View::u32_le_at(bytes, count_at + 1)?).ok()?;
    let body_count = scope.reference_members.len().checked_sub(1)?;
    if reference_count != body_count.checked_mul(2)? {
        return None;
    }
    let mut bodies = Vec::with_capacity(body_count);
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
        View::u32_le_at(bytes, at + 1)
    };
    for (ordinal, operand) in operands.into_iter().enumerate() {
        let source_at = references_at.checked_add(ordinal.checked_mul(30)?)?;
        let copied_at = source_at.checked_add(15)?;
        bodies.push(crate::records::DesignCopiedBody {
            operand,
            source: crate::records::Located { value: body_reference(source_at, 10)?, offset: u64::try_from(source_at + 1).ok()? },
            copied: crate::records::Located { value: body_reference(copied_at, if ordinal + 1 == body_count { 6 } else { 10 })?, offset: u64::try_from(copied_at + 1).ok()? },
        });
    }
    if bodies.iter().flat_map(|body| [body.source.value, body.copied.value])
        .collect::<HashSet<_>>()
        .len()
        != reference_count
    {
        return None;
    }
    Some(DesignCopyPasteBodiesOperation {
        bodies,
        body_group_record_index,
        body_group_class_tag,
        body_group_byte_offset: u64::try_from(body_group_at).ok()?,
        relation_record_index,
        relation_class_tag,
        relation_byte_offset: u64::try_from(relation_at).ok()?,
    })
}

pub(crate) fn exact_base_feature_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignBaseFeatureConstruction> {
    use crate::records::{DesignBaseFeatureEntry, DesignBaseFeatureResultBody, DesignBaseFeatureResults};
    if scope.kind() != crate::records::DesignFeatureKind::BaseFeature {
        return None;
    }
    if let Some(snapshot) = exact_base_feature_body_snapshot(bytes, scope) {
        return Some(snapshot);
    }
    if let Some(body_based_on_faces) =
        base_feature::exact_base_feature_body_based_on_faces(bytes, scope)
    {
        return Some(body_based_on_faces);
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if scope.frame_length == 267 {
        return Some(DesignBaseFeatureConstruction::ResultBodies {
            bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
            metadata_record: View::u32_le_at(bytes, usize::try_from(scope.byte_offset).ok()? + 37)?,
            metadata_record_offset: scope.byte_offset + 37,
            metadata_field: bytes.get(start + 45..start + 51)?.to_vec(),
        });
    }
    let legacy_290_261 = scope.class_tag == "290" && scope.paired_class_tag == "261";
    let legacy_360_258 = scope.class_tag == "360" && scope.paired_class_tag == "258";
    let legacy_409_262 = scope.class_tag == "409" && scope.paired_class_tag == "262";
    let legacy_444_263 = scope.class_tag == "444" && scope.paired_class_tag == "263";
    if legacy_409_262 && scope.frame_length == 258 {
        if scope.byte_offset.checked_add(scope.frame_length) != Some(scope.paired_byte_offset) {
            return None;
        }
        let metadata_record = u32::try_from(View::u64_le_at(
            bytes,
            start + legacy_zero_body::SHARED_METADATA_RECORD,
        )?)
        .ok()?;
        if bytes
            .get(start + legacy_zero_body::ZERO_RUN_9..start + legacy_zero_body::ZERO_BODY_MARKER)?
            != [0; 9]
            || bytes.get(start + legacy_zero_body::ZERO_BODY_MARKER) != Some(&1)
            || bytes.get(
                start + legacy_zero_body::ZERO_RUN_11
                    ..start + legacy_zero_body::SHARED_METADATA_MARKER,
            )? != [0; 11]
            || bytes.get(start + legacy_zero_body::SHARED_METADATA_MARKER) != Some(&1)
            || !scope.reference_members.values().copied().eq([metadata_record])
        {
            return None;
        }
        let uuid_offset = usize::try_from(scope.kind_offset).ok()?.checked_sub(102)?;
        if uuid_offset < start + legacy_zero_body::ZERO_PADDING_8 {
            return None;
        }
        if !bytes
            .get(start + legacy_zero_body::ZERO_PADDING_8..uuid_offset)?
            .iter()
            .all(|byte| *byte == 0)
        {
            return None;
        }
        return Some(DesignBaseFeatureConstruction::ResultBodies {
            bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
            metadata_record,
            metadata_record_offset: scope.byte_offset
                + u64::try_from(legacy_zero_body::SHARED_METADATA_RECORD).ok()?,
            metadata_field: bytes
                .get(
                    start + legacy_zero_body::SHARED_METADATA_FIELD
                        ..start + legacy_zero_body::ZERO_PADDING_8,
                )?
                .to_vec(),
        });
    }
    if legacy_444_263 && scope.frame_length == 258 {
        if scope.byte_offset.checked_add(scope.frame_length) != Some(scope.paired_byte_offset)
            || scope.reference_members.len() != 1
            || scope.reference_count_offset
                != scope.byte_offset + u64::try_from(legacy_444_zero_body::REFERENCE_COUNT).ok()?
            || !scope.reference_members.offsets().copied().eq([scope.byte_offset
                    + u64::try_from(legacy_444_zero_body::SCOPE_REFERENCE_RECORD).ok()?])
            || scope.kind_offset
                != scope.byte_offset + u64::try_from(legacy_444_zero_body::KIND_LENGTH + 4).ok()?
        {
            return None;
        }
        let metadata_record = u32::try_from(View::u64_le_at(
            bytes,
            start + legacy_444_zero_body::SHARED_METADATA_RECORD,
        )?)
        .ok()?;
        let guid_code_units =
            usize::try_from(legacy_444_zero_body::GUID_CODE_UNIT_COUNT_VALUE).ok()?;
        let (guid, guid_end) = lp_utf16_bounded(
            bytes,
            start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
            guid_code_units..=guid_code_units,
        )?;
        if bytes.get(
            start + legacy_444_zero_body::ZERO_RUN_9
                ..start + legacy_444_zero_body::ZERO_BODY_MARKER,
        )? != [0; 9]
            || bytes.get(start + legacy_444_zero_body::ZERO_BODY_MARKER)
                != Some(&legacy_444_zero_body::ZERO_BODY_MARKER_VALUE)
            || bytes.get(
                start + legacy_444_zero_body::ZERO_RUN_11
                    ..start + legacy_444_zero_body::SHARED_METADATA_MARKER,
            )? != [0; 11]
            || bytes.get(start + legacy_444_zero_body::SHARED_METADATA_MARKER)
                != Some(&legacy_444_zero_body::SHARED_METADATA_MARKER_VALUE)
            || !scope.reference_members.values().copied().eq([metadata_record])
            || bytes.get(
                start + legacy_444_zero_body::SHARED_METADATA_ZERO_TAIL
                    ..start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
            )? != [0; 14]
            || !is_guid_relaxed(&guid)
            || guid_end != start + legacy_444_zero_body::ZERO_RUN_3
            || bytes.get(
                start + legacy_444_zero_body::ZERO_RUN_3
                    ..start + legacy_444_zero_body::REFERENCE_COUNT,
            )? != [0; 3]
            || View::u32_le_at(bytes, start + legacy_444_zero_body::REFERENCE_COUNT)?
                != legacy_444_zero_body::REFERENCE_COUNT_VALUE
            || bytes.get(start + legacy_444_zero_body::SCOPE_REFERENCE_MARKER)
                != Some(&legacy_444_zero_body::SCOPE_REFERENCE_MARKER_VALUE)
            || View::u32_le_at(bytes, start + legacy_444_zero_body::SCOPE_REFERENCE_RECORD)?
                != metadata_record
            || bytes.get(
                start + legacy_444_zero_body::SCOPE_REFERENCE_FIELD
                    ..start + legacy_444_zero_body::HISTORY_STATE_ID,
            )? != [0; 6]
            || View::u32_le_at(bytes, start + legacy_444_zero_body::KIND_LENGTH)?
                != legacy_444_zero_body::KIND_LENGTH_VALUE
        {
            return None;
        }
        return Some(DesignBaseFeatureConstruction::ResultBodies {
            bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
            metadata_record,
            metadata_record_offset: scope.byte_offset
                + u64::try_from(legacy_444_zero_body::SHARED_METADATA_RECORD).ok()?,
            metadata_field: bytes
                .get(
                    start + legacy_444_zero_body::SHARED_METADATA_ZERO_TAIL
                        ..start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
                )?
                .to_vec(),
        });
    }
    if bytes.get(start + result_body::ZERO_RUN_8..start + result_body::BODY_COUNT_MARKER)? != [0; 8]
        || bytes.get(start + result_body::BODY_COUNT_MARKER) != Some(&1)
    {
        return None;
    }
    let combined_count = usize::try_from(View::u32_le_at(
        bytes,
        start + result_body::COMBINED_BODY_REFERENCE_COUNT,
    )?)
    .ok()?;
    if combined_count == 0 || combined_count > 200_000 || combined_count % 2 != 0 {
        return None;
    }
    let body_count = combined_count / 2;
    let expanded = legacy_290_261
        || legacy_360_258
        || matches!(
            (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
            ("384", "264") | ("409", "262")
        );
    let compact = matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("420", "258") | ("452", "266")
    );
    let base_length = if legacy_290_261 {
        261
    } else if expanded || compact || legacy_444_263 {
        262
    } else {
        271
    };
    if scope.frame_length != base_length + u64::try_from(body_count.checked_mul(52)?).ok()? {
        return None;
    }
    let mut cursor = start + result_body::LEN;
    let mut read_u64_run = |count: usize| {
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(cursor) != Some(&1) {
                return None;
            }
            entries.push(DesignBaseFeatureEntry {
                value: View::u64_le_at(bytes, cursor + result_body_entry::REFERENCE_VALUE)?,
                offset: u64::try_from(cursor + result_body_entry::REFERENCE_VALUE).ok()?,
                field: bytes.get(cursor + result_body_entry::REFERENCE_FIELD..cursor + result_body_entry::LEN)?.try_into().ok()?,
            });
            cursor += result_body_entry::LEN;
        }
        Some(entries)
    };
    let entities = read_u64_run(body_count)?;
    let references = read_u64_run(body_count)?.into_iter().map(|entry| {
        Some(DesignBaseFeatureEntry { value: u32::try_from(entry.value).ok()?, offset: entry.offset, field: entry.field })
    }).collect::<Option<Vec<_>>>()?;
    if expanded {
        if bytes.get(cursor) != Some(&1)
            || bytes.get(cursor + 1..cursor + 7) != Some(&[0; 6])
            || usize::try_from(View::u32_le_at(bytes, cursor + 7)?).ok()? != body_count
        {
            return None;
        }
        cursor += 11;
    } else if legacy_444_263 {
        if bytes.get(cursor + compact_count::COUNT_MARKER) != Some(&1)
            || bytes.get(cursor + compact_count::ZERO_RUN_5..cursor + compact_count::REPEAT_MARKER)
                != Some(&[0; 5])
            || bytes.get(cursor + compact_count::REPEAT_MARKER) != Some(&0)
            || usize::try_from(View::u32_le_at(bytes, cursor + compact_count::BODY_COUNT)?).ok()?
                != body_count
        {
            return None;
        }
        cursor += compact_count::LEN;
    } else if compact {
        if bytes.get(cursor + compact_count::COUNT_MARKER) != Some(&1)
            || bytes.get(cursor + compact_count::ZERO_RUN_5..cursor + compact_count::REPEAT_MARKER)
                != Some(&[0; 5])
            || bytes.get(cursor + compact_count::REPEAT_MARKER) != Some(&1)
            || usize::try_from(View::u32_le_at(bytes, cursor + compact_count::BODY_COUNT)?).ok()?
                != body_count
        {
            return None;
        }
        cursor += compact_count::LEN;
    } else {
        if bytes.get(cursor) != Some(&1) || bytes.get(cursor + 1..cursor + 11) != Some(&[0; 10]) {
            return None;
        }
        cursor += 11;
        if usize::try_from(View::u32_le_at(bytes, cursor)?).ok()? != body_count {
            return None;
        }
        cursor += 4;
    }
    let mut repeated_reference_fields = Vec::with_capacity(body_count);
    for ordinal in 0..body_count {
        let expected = if compact {
            u32::try_from(entities[ordinal].value).ok()?
        } else {
            references[ordinal].value
        };
        if bytes.get(cursor + compact_entry::BODY_MARKER) != Some(&1)
            || View::u32_le_at(bytes, cursor + compact_entry::BODY_ENTITY_SUFFIX)? != expected
        {
            return None;
        }
        repeated_reference_fields.push(
            bytes
                .get(cursor + compact_entry::BODY_FIELD..cursor + compact_entry::LEN)?
                .try_into()
                .ok()?,
        );
        cursor += compact_entry::LEN;
    }
    if bytes.get(cursor) != Some(&0) {
        return None;
    }
    cursor += 1;
    if bytes.get(cursor) != Some(&1) {
        return None;
    }
    let metadata_record = u32::try_from(View::u64_le_at(bytes, cursor + 1)?).ok()?;
    let metadata_record_offset = u64::try_from(cursor + 1).ok()?;
    let metadata_field_width = if expanded || compact || legacy_444_263 {
        2
    } else {
        6
    };
    let metadata_field = bytes
        .get(cursor + 9..cursor + 9 + metadata_field_width)?
        .to_vec();
    cursor += 9 + metadata_field_width;
    if usize::try_from(View::u32_le_at(bytes, cursor)?).ok()? != body_count {
        return None;
    }
    cursor += 4;
    let mut result_rows = Vec::with_capacity(body_count);
    for ((entity, reference), field) in entities.into_iter().zip(references).zip(repeated_reference_fields) {
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        let result = DesignBaseFeatureEntry {
            value: View::u32_le_at(bytes, cursor + 1)?,
            offset: u64::try_from(cursor + 1).ok()?,
            field: bytes.get(cursor + 5..cursor + 11)?.try_into().ok()?,
        };
        result_rows.push((DesignBaseFeatureResultBody { entity, reference, result }, field));
        cursor += 11;
    }
    let uuid_offset = usize::try_from(scope.kind_offset).ok()?.checked_sub(102)?;
    let admitted = cursor <= uuid_offset
        && bytes
            .get(cursor..uuid_offset)
            .is_some_and(|padding| padding.iter().all(|byte| *byte == 0));
    let mut result_rows = result_rows.into_iter();
    let first = result_rows.next()?;
    admitted.then_some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithRepeatedFields { first, rest: result_rows.collect() },
        metadata_record,
        metadata_record_offset,
        metadata_field,
    })
}

fn exact_base_feature_body_snapshot(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignBaseFeatureConstruction> {
    // Fixed prefix, linkage and GUID blocks, generic scope prefix, kind
    // prefix, ordinal, and closing tail; the kind payload adds 2L bytes.
    const FIXED_FRAME_LENGTH: u64 = 431;
    if scope.class_tag != "314"
        || scope.paired_class_tag != "259"
        || scope.reference_members.len() != 1
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let body_count = usize::try_from(View::u32_le_at(bytes, start + snapshot::BODY_COUNT)?).ok()?;
    let kind_width = scope.kind_name().encode_utf16().count().checked_mul(2)?;
    let expected_frame_length = FIXED_FRAME_LENGTH
        .checked_add(u64::try_from(body_count.checked_mul(snapshot_entry::LEN)?).ok()?)?
        .checked_add(u64::try_from(kind_width).ok()?)?;
    if !(1..=200_000).contains(&body_count)
        || scope.frame_length != expected_frame_length
        || bytes.get(start + snapshot::ZERO_RUN_8..start + snapshot::BODY_COUNT_MARKER)? != [0; 8]
        || bytes.get(start + snapshot::BODY_COUNT_MARKER) != Some(&1)
    {
        return None;
    }
    let mut cursor = start + snapshot::LEN;
    let mut bodies = Vec::with_capacity(body_count);
    for _ in 0..body_count {
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        bodies.push(crate::records::DesignBaseFeatureEntry {
            value: View::u64_le_at(bytes, cursor + snapshot_entry::BODY_ENTITY_SUFFIX)?,
            offset: u64::try_from(cursor + snapshot_entry::BODY_ENTITY_SUFFIX).ok()?,
            field: bytes.get(cursor + snapshot_entry::BODY_ENTITY_FIELD..cursor + snapshot_entry::LEN)?.try_into().ok()?,
        });
        cursor += snapshot_entry::LEN;
    }
    let preamble = bytes.get(cursor..cursor + snapshot_expanded_preamble::LEN)?;
    let packed_guid_preamble = if preamble == [1, 0, 0, 0, 0, 1, 0, 0, 0] {
        true
    } else if preamble[..snapshot_compact_preamble::LEN] == [1, 0, 0, 0, 1, 0, 0, 0] {
        false
    } else {
        return None;
    };
    cursor += if packed_guid_preamble {
        snapshot_expanded_preamble::LEN
    } else {
        snapshot_compact_preamble::LEN
    };
    let parse_guid = |at: usize| {
        let (guid, end) = lp_utf16_bounded(bytes, at, 36..=36)?;
        crate::bytes::is_guid_relaxed(&guid).then_some((guid, end, at + snapshot_guid::GUID_UTF16))
    };
    let (first_guid, after_first_guid, first_guid_offset) = parse_guid(cursor)?;
    let (second_guid, after_second_guid, second_guid_offset) = parse_guid(after_first_guid)?;
    // The nine-byte preamble carries the linkage anchor in the final zero
    // byte of the second GUID's UTF-16 payload. Keep the full GUID for the
    // native record, but anchor the fixed tail at that shared byte.
    let after_guids = if packed_guid_preamble {
        after_second_guid.checked_sub(1)?
    } else {
        after_second_guid
    };
    if bytes.get(after_guids..after_guids + snapshot_tail::FIRST_BODY_MARKER)?
        != [0, 0, 1, 1, 0, 0, 0]
        || bytes.get(after_guids + snapshot_tail::FIRST_BODY_MARKER) != Some(&1)
        || View::u64_le_at(bytes, after_guids + snapshot_tail::FIRST_BODY_ENTITY_SUFFIX)?
            != bodies.first()?.value
        || bytes.get(
            after_guids + snapshot_tail::ZERO_RUN_3..after_guids + snapshot_tail::LINKAGE_MARKER,
        )? != [0; 3]
        || bytes.get(after_guids + snapshot_tail::LINKAGE_MARKER) != Some(&1)
    {
        return None;
    }
    let linkage_record = u32::try_from(View::u64_le_at(
        bytes,
        after_guids + snapshot_tail::LINKAGE_RECORD,
    )?)
    .ok()?;
    if linkage_record != *scope.reference_members.values().next()?
        || bytes.get(
            after_guids + snapshot_tail::ZERO_RUN_6..after_guids + snapshot_tail::RELATION_COUNT,
        )? != [0; 6]
        || View::u32_le_at(bytes, after_guids + snapshot_tail::RELATION_COUNT)? != 1
        || bytes.get(after_guids + snapshot_tail::AUXILIARY_MARKER) != Some(&1)
    {
        return None;
    }
    let auxiliary_record = u32::try_from(View::u64_le_at(
        bytes,
        after_guids + snapshot_tail::AUXILIARY_RECORD,
    )?)
    .ok()?;
    if bytes.get(
        after_guids + snapshot_tail::TRAILING_ZERO_RUN_6
            ..after_guids + snapshot_tail::TRAILING_ZERO_RUN_4,
    )? != [0; 6]
        || bytes.get(
            after_guids + snapshot_tail::TRAILING_ZERO_RUN_4..after_guids + snapshot_tail::LEN,
        )? != [0; 4]
    {
        return None;
    }
    let (third_guid, after_third_guid, third_guid_offset) =
        parse_guid(after_guids + snapshot_tail::LEN)?;
    let reference_count_at = after_third_guid.checked_add(snapshot_scope::REFERENCE_COUNT)?;
    let reference_marker = after_third_guid.checked_add(snapshot_scope::REFERENCE_MARKER)?;
    let state_at = after_third_guid.checked_add(snapshot_scope::HISTORY_STATE_ID)?;
    let kind_at = after_third_guid.checked_add(snapshot_scope::KIND_CODE_UNIT_COUNT)?;
    if bytes.get(after_third_guid..reference_count_at)? != [0; 3]
        || View::u32_le_at(bytes, reference_count_at)? != 1
        || bytes.get(reference_marker) != Some(&1)
        || View::u32_le_at(bytes, reference_marker + 1)? != *scope.reference_members.values().next()?
        || bytes.get(reference_marker + 5..state_at)? != [0; 6]
        || scope.reference_count_offset != u64::try_from(reference_count_at).ok()?
        || scope.reference_members.offsets().next().copied()
            != Some(u64::try_from(reference_marker + 1).ok()?)
        || scope.kind_offset != u64::try_from(kind_at + 4).ok()?
    {
        return None;
    }
    match scope.history_state_id {
        Some(history_state_id)
            if View::u32_le_at(bytes, state_at)? != u32::try_from(history_state_id).ok()? =>
        {
            return None;
        }
        None if View::u32_le_at(bytes, state_at)? != u32::MAX => return None,
        _ => {}
    }
    let (kind, kind_end) = lp_utf16_bounded(bytes, kind_at, 1..=256)?;
    if kind != scope.kind_name()
        || View::u32_le_at(bytes, kind_end)? != scope.feature_ordinal.get()
        || scope.feature_ordinal_offset != u64::try_from(kind_end).ok()?
        || scope.previous_history_state_id.is_some()
        || scope.previous_history_state_id_offset.is_some()
    {
        return None;
    }
    if scope.paired_byte_offset
        != u64::try_from(start.checked_add(usize::try_from(scope.frame_length).ok()?)?).ok()?
    {
        return None;
    }
    Some(DesignBaseFeatureConstruction::BodySnapshot {
        bodies,
        related_guids: [first_guid, second_guid, third_guid],
        related_guid_offsets: [
            u64::try_from(first_guid_offset).ok()?,
            u64::try_from(second_guid_offset).ok()?,
            u64::try_from(third_guid_offset).ok()?,
        ],
        linkage_record,
        linkage_record_offset: u64::try_from(after_guids + snapshot_tail::LINKAGE_RECORD).ok()?,
        auxiliary_record,
        auxiliary_record_offset: u64::try_from(after_guids + snapshot_tail::AUXILIARY_RECORD)
            .ok()?,
    })
}

pub(crate) fn exact_solid_primitive(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignSolidPrimitive> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let (operation, operation_offset, cylinder_transform) = match scope
        .kind_name()
    {
        "SpherePrimitive" | "TorusPrimitive" => {
            let operation_offset = start.checked_add(25)?;
            (
                primitive_operation(bytes, operation_offset)?,
                operation_offset,
                None,
            )
        }
        "BoxPrimitive" => {
            let operation_offset = exact_named_solid_primitive_operation(bytes, start)?;
            (
                primitive_operation(bytes, operation_offset)?,
                operation_offset,
                None,
            )
        }
        "CylinderPrimitive" => {
            if let Some(operation_offset) = exact_named_solid_primitive_operation(bytes, start) {
                (
                    primitive_operation(bytes, operation_offset)?,
                    operation_offset,
                    None,
                )
            } else {
                let prologue = exact_shifted_cylinder_primitive_prologue(bytes, scope, start)?;
                (
                    prologue.operation,
                    prologue.operation_offset,
                    prologue.transform,
                )
            }
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
    match scope.kind_name() {
        "SpherePrimitive"
            if scope.frame_length == 462
                && bytes.get(start + 29) == Some(&1)
                && bytes.get(start + 30) == Some(&1)
                && bytes.get(start + 41) == Some(&1)
                && bytes.get(start + 52) == Some(&1) =>
        {
            let diameter_record_index = View::u32_le_at(bytes, start + 42)?;
            let (diameter, diameter_offset) =
                exact_primitive_diameter(bytes, records, diameter_record_index)?;
            let (transform, transform_offset) = matrix(64)?;
            Some(DesignSolidPrimitive::Sphere(crate::records::DesignSpherePrimitive {
                transform,
                transform_offset,
                diameter,
                diameter_record_index,
                diameter_offset,
                operation,
                operation_offset: operation_offset as u64,
            }))
        }
        "TorusPrimitive"
            if scope.frame_length == 486
                && bytes.get(start + 29) == Some(&1)
                && bytes.get(start + 30) == Some(&1)
                && bytes.get(start + 41) == Some(&1)
                && bytes.get(start + 52) == Some(&1)
                && bytes.get(start + 63) == Some(&1) =>
        {
            let major_diameter_record_index = View::u32_le_at(bytes, start + 31)?;
            let minor_diameter_record_index = View::u32_le_at(bytes, start + 53)?;
            if major_diameter_record_index == minor_diameter_record_index {
                return None;
            }
            let (major_diameter, major_diameter_offset) =
                exact_primitive_diameter(bytes, records, major_diameter_record_index)?;
            let (minor_diameter, minor_diameter_offset) =
                exact_primitive_diameter(bytes, records, minor_diameter_record_index)?;
            let (transform, transform_offset) = matrix(75)?;
            Some(DesignSolidPrimitive::Torus(crate::records::DesignTorusPrimitive {
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
            }))
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
                .then_some(DesignSolidPrimitive::Box(crate::records::DesignBoxPrimitive {
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
                }))
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
                DesignSolidPrimitive::Cylinder(crate::records::DesignCylinderPrimitive {
                    height: height.evaluated_value,
                    height_record_index: height.record_index,
                    height_offset: height.evaluated_value_offset,
                    diameter: diameter.evaluated_value,
                    diameter_record_index: diameter.record_index,
                    diameter_offset: diameter.evaluated_value_offset,
                    transform: cylinder_transform,
                    operation,
                    operation_offset: operation_offset as u64,
                }),
            )
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct ExactShiftedCylinderPrimitivePrologue {
    operation: DesignExtrudeOperation,
    operation_offset: usize,
    transform: Option<crate::records::Located<[[f64; 4]; 4]>>,
}

fn exact_named_solid_primitive_operation(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start + solid_prologue::ZERO_RUN_9..start + solid_prologue::OPERATION)? != [0; 9]
        || bytes.get(start + solid_prologue::ZERO_FLAG) != Some(&0)
        || bytes.get(start + solid_prologue::FORM_MARKER) != Some(&1)
    {
        return None;
    }
    start.checked_add(solid_prologue::OPERATION)
}

fn exact_shifted_cylinder_primitive_prologue(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<ExactShiftedCylinderPrimitivePrologue> {
    let compact = match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length,
    ) {
        ("297" | "375", "258", 352) => true,
        ("297" | "375", "258", 502) | ("414", "272", 502) => false,
        _ => return None,
    };
    let (
        frame_length,
        zero_run_10,
        form_marker,
        operation,
        first_reference,
        second_reference,
        third_reference,
        fourth_reference,
        reference_gap,
    ) = if compact {
        (
            shifted_cylinder_352::LEN,
            shifted_cylinder_352::ZERO_RUN_10,
            shifted_cylinder_352::FORM_MARKER,
            shifted_cylinder_352::OPERATION,
            shifted_cylinder_352::FIRST_REFERENCE,
            shifted_cylinder_352::SECOND_REFERENCE,
            shifted_cylinder_352::THIRD_REFERENCE,
            shifted_cylinder_352::FOURTH_REFERENCE,
            shifted_cylinder_352::REFERENCE_GAP,
        )
    } else {
        (
            shifted_cylinder_502::LEN,
            shifted_cylinder_502::ZERO_RUN_10,
            shifted_cylinder_502::FORM_MARKER,
            shifted_cylinder_502::OPERATION,
            shifted_cylinder_502::FIRST_REFERENCE,
            shifted_cylinder_502::SECOND_REFERENCE,
            shifted_cylinder_502::THIRD_REFERENCE,
            shifted_cylinder_502::FOURTH_REFERENCE,
            shifted_cylinder_502::REFERENCE_GAP,
        )
    };
    let reference_count = if compact { 5 } else { 7 };
    if scope.reference_members.len() != reference_count
        || scope.paired_byte_offset != u64::try_from(start.checked_add(frame_length)?).ok()?
        || bytes.get(start + zero_run_10..start + form_marker)? != [0; 10]
        || bytes.get(start + form_marker) != Some(&1)
        || bytes.get(start + reference_gap) != Some(&0)
        || bytes.get(start + operation + 1..start + first_reference)? != [0; 3]
    {
        return None;
    }
    let operation_offset = start.checked_add(operation)?;
    let operation = primitive_operation(bytes, operation_offset)?;
    if bytes.get(start + first_reference) != Some(&1)
        || bytes.get(start + first_reference + 1) != Some(&1)
        || View::u32_le_at(bytes, start + first_reference + 2)?
            != *scope.reference_members.values().nth(reference_count - 1)?
        || bytes.get(start + first_reference + 6..start + first_reference + 11)? != [0; 5]
    {
        return None;
    }
    for (relative_offset, expected_record_index) in [
        (
            second_reference,
            *scope.reference_members.values().nth(reference_count - 2)?,
        ),
        (
            third_reference,
            *scope.reference_members.values().nth(reference_count - 3)?,
        ),
        (
            fourth_reference,
            *scope.reference_members.values().nth(reference_count - 4)?,
        ),
    ] {
        if marked_record_reference(bytes, start.checked_add(relative_offset)?)
            != Some(expected_record_index)
        {
            return None;
        }
    }
    let absolute = |relative_offset: usize| u64::try_from(start.checked_add(relative_offset)?).ok();
    let (
        reference_count_offset,
        kind_offset,
        feature_ordinal_offset,
        previous_history_state_id_offset,
    ) = match scope.frame_length {
        352 => (
            shifted_cylinder_352::REFERENCE_COUNT,
            shifted_cylinder_352::KIND,
            shifted_cylinder_352::FEATURE_ORDINAL,
            shifted_cylinder_352::PREVIOUS_HISTORY_STATE_ID,
        ),
        502 => (
            shifted_cylinder_502::REFERENCE_COUNT,
            shifted_cylinder_502::KIND,
            shifted_cylinder_502::FEATURE_ORDINAL,
            shifted_cylinder_502::PREVIOUS_HISTORY_STATE_ID,
        ),
        _ => return None,
    };
    if scope.reference_count_offset != absolute(reference_count_offset)?
        || scope.kind_offset != absolute(kind_offset)?
        || scope.feature_ordinal_offset != absolute(feature_ordinal_offset)?
        || scope.previous_history_state_id_offset
            != Some(absolute(previous_history_state_id_offset)?)
    {
        return None;
    }
    let transform = match scope.frame_length {
        352 => {
            if bytes.get(start + shifted_cylinder_352::COMPACT_TAIL_MARKER) != Some(&1)
                || View::u32_le_at(bytes, start + shifted_cylinder_352::COMPACT_TAIL_COUNT)? != 1
                || marked_record_reference(
                    bytes,
                    start + shifted_cylinder_352::COMPACT_TAIL_REFERENCE,
                )
                .is_none_or(|record_index| record_index == 0)
                || bytes.get(
                    start + shifted_cylinder_352::COMPACT_TAIL_ZERO_RUN_8
                        ..start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT,
                )? != [0; 8]
                || View::u32_le_at(bytes, start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT)? != 36
            {
                return None;
            }
            let (guid, guid_end) = lp_utf16_bounded(
                bytes,
                start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT,
                36..=36,
            )?;
            if guid_end != start + shifted_cylinder_352::ZERO_RUN_3_AFTER_GUID
                || !is_guid_relaxed(&guid)
                || bytes.get(
                    start + shifted_cylinder_352::ZERO_RUN_3_AFTER_GUID
                        ..start + shifted_cylinder_352::REFERENCE_COUNT,
                )? != [0; 3]
            {
                return None;
            }
            None
        }
        502 => {
            if bytes.get(start + shifted_cylinder_502::ZERO_BEFORE_MATRIX) != Some(&0)
                || bytes.get(
                    start + shifted_cylinder_502::ZERO_RUN_8_AFTER_MATRIX
                        ..start + shifted_cylinder_502::CONSTRUCTION_REFERENCE,
                )? != [0; 8]
                || bytes.get(start + shifted_cylinder_502::CONSTRUCTION_REFERENCE) != Some(&1)
                || View::u32_le_at(
                    bytes,
                    start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 1,
                )? != 0x0100_0000
                || View::u32_le_at(
                    bytes,
                    start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 5,
                )? != *scope.reference_members.values().next()?
                || bytes.get(
                    start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 9
                        ..start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT,
                )? != [0; 6]
                || View::u32_le_at(bytes, start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT)? != 36
                || bytes.get(
                    start + shifted_cylinder_502::ZERO_RUN_3_AFTER_GUID
                        ..start + shifted_cylinder_502::REFERENCE_COUNT,
                )? != [0; 3]
            {
                return None;
            }
            let values = f64s_at(bytes, start + shifted_cylinder_502::MATRIX, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            let (guid, guid_end) = lp_utf16_bounded(
                bytes,
                start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT,
                36..=36,
            )?;
            if guid_end != start + shifted_cylinder_502::ZERO_RUN_3_AFTER_GUID
                || !is_guid_relaxed(&guid)
                || !valid_sketch_transform(&transform)
                || !cylinder_transform_preserves_projected_geometry(&transform)
            {
                return None;
            }
            Some(crate::records::Located {
                value: transform,
                offset: u64::try_from(start + shifted_cylinder_502::MATRIX).ok()?,
            })
        }
        _ => return None,
    };
    Some(ExactShiftedCylinderPrimitivePrologue {
        operation,
        operation_offset,
        transform,
    })
}

fn cylinder_transform_preserves_projected_geometry(transform: &[[f64; 4]; 4]) -> bool {
    const EPS_CYLINDER_FRAME: f64 = 1.0e-10;
    transform[0][3].abs() <= EPS_CYLINDER_FRAME
        && transform[1][3].abs() <= EPS_CYLINDER_FRAME
        && transform[2][3].abs() <= EPS_CYLINDER_FRAME
        && transform[0][2].abs() <= EPS_CYLINDER_FRAME
        && transform[1][2].abs() <= EPS_CYLINDER_FRAME
        && (transform[2][2] - 1.0).abs() <= EPS_CYLINDER_FRAME
}

fn primitive_operation(bytes: &[u8], offset: usize) -> Option<DesignExtrudeOperation> {
    match View::u32_le_at(bytes, offset)? {
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
                && scope.reference_members.values().any(|value| value == &owner.record_index)
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
                        || marked_record_reference(bytes, start + 67)
                            != View::u32_le_at(bytes, start + 25)
                        || bytes.get(start + 78..start + 80) != Some(&[0; 2])
                        || marked_record_reference(bytes, start + 80).is_none()
                        || bytes.get(start + 85..start + 92) != Some(&[0; 7])
                        || marked_record_reference(bytes, start + 92)
                            != View::u32_le_at(bytes, start + 25))
                {
                    return None;
                }
            }
            let value = View::f64_le_at(bytes, start + 40)?;
            value.is_finite().then_some(FixedScalarFrame {
                owner_record_index: (bytes.get(start + 24) == Some(&1))
                    .then(|| View::u32_le_at(bytes, start + 25))
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

fn exact_pipe_owner_lanes(
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<[(u32, FixedScalarFrame); 4]> {
    let stream = native_stream(&scope.id)?;
    let mut owners = parameter_owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(stream)
                && owner.scope_record_index == scope.record_index
                && scope.reference_members.values().any(|value| value == &owner.record_index)
                && owner.class_tag == "342"
                && owner.frame_length == 103
                && owner.evaluated_value.is_finite()
        })
        .collect::<Vec<_>>();
    owners.sort_by_key(|owner| owner.local_ordinal);
    if owners.len() != 4
        || owners
            .iter()
            .enumerate()
            .any(|(ordinal, owner)| owner.local_ordinal != ordinal as u32)
    {
        return None;
    }
    owners
        .into_iter()
        .map(|owner| {
            Some((
                owner.record_index,
                FixedScalarFrame {
                    owner_record_index: Some(scope.record_index),
                    ordinal: u8::try_from(owner.local_ordinal).ok()?,
                    value: owner.evaluated_value,
                    value_offset: owner.evaluated_value_offset,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

/// Decode class-347/258 Thicken, whose group precedes its scalar. The class
/// pair and 291-byte frame are part of admission for this distinct grammar.
fn exact_legacy_thicken_class_347(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag != "347"
        || scope.paired_class_tag != "258"
        || scope.frame_length != u64::try_from(thicken_347::LEN).ok()?
        || scope.reference_members.len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if bytes.get(start + thicken_347::ZERO_RUN_10..start + thicken_347::FEATURE_FORM)
        != Some(&[0; 10])
        || View::u32_le_at(bytes, start + thicken_347::FEATURE_FORM)? != 4
        || View::u32_le_at(bytes, start + thicken_347::GROUP_FORM)? != 1
        || marked_record_reference(bytes, start + thicken_347::GROUP_REFERENCE)?
            != scope.reference_members.values().next().copied()?
        || bytes.get(start + thicken_347::SCALAR_PREFIX..start + thicken_347::SCALAR_REFERENCE)
            != Some(&[1, 1])
        || View::u32_le_at(bytes, start + thicken_347::AUXILIARY_COUNT)? != 1
        || marked_record_reference(bytes, start + thicken_347::AUXILIARY_REFERENCE).is_none()
        || bytes.get(start + thicken_347::ZERO_RUN_8..start + thicken_347::GUID_CODE_UNIT_COUNT)
            != Some(&[0; 8])
        || View::u32_le_at(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)? != 36
        || bytes.get(start + thicken_347::ZERO_RUN_3..start + thicken_347::REFERENCE_COUNT)
            != Some(&[0; 3])
        || View::u32_le_at(bytes, start + thicken_347::REFERENCE_COUNT)? != 3
        || View::u32_le_at(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT)? != 7
    {
        return None;
    }
    let (guid, guid_end) =
        lp_utf16_bounded(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT, 36..=36)?;
    if guid_end != start + thicken_347::ZERO_RUN_3 || !is_guid_relaxed(&guid) {
        return None;
    }
    let (kind, kind_end) =
        lp_utf16_bounded(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT, 7..=7)?;
    if kind != "Thicken" || kind_end != start + thicken_347::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        thicken_347::GROUP_REFERENCE_ENTRY,
        thicken_347::MEMBER_REFERENCE_ENTRY,
        thicken_347::SCALAR_REFERENCE_ENTRY,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members.values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index =
        marked_record_reference(bytes, start + thicken_347::SCALAR_REFERENCE)?;
    if scope.reference_members.values().next_back().copied()? != thickness_record_index {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    (scalar.value != 0.0).then_some(DesignDirectFaceOperation::Thicken(crate::records::DesignThickenOperation {
        signed_thickness: scalar.value,
        thickness_record_index,
        thickness_offset: scalar.value_offset,
    }))
}

fn exact_shell_class_369_261(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag != "369"
        || scope.paired_class_tag != "261"
        || scope.frame_length != shell_369_261::LEN as u64
        || scope.reference_members.len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if bytes.get(start + shell_369_261::ZERO_RUN_9..start + shell_369_261::FEATURE_FORM)
        != Some(&[0; 9])
        || bytes.get(start + shell_369_261::FEATURE_FORM)
            != Some(&shell_369_261::FEATURE_FORM_VALUE)
        || bytes.get(start + shell_369_261::ZERO_RUN_3..start + shell_369_261::SCALAR_MARKER)
            != Some(&[0; 3])
        || bytes.get(start + shell_369_261::SCALAR_MARKER)
            != Some(&shell_369_261::SCALAR_MARKER_VALUE)
        || bytes
            .get(start + shell_369_261::ZERO_RUN_9_AFTER_SCALAR..start + shell_369_261::GROUP_FORM)
            != Some(&[0; 9])
        || bytes.get(start + shell_369_261::GROUP_FORM) != Some(&shell_369_261::GROUP_FORM_VALUE)
        || bytes.get(start + 47..start + shell_369_261::GROUP_REFERENCE) != Some(&[0; 3])
        || bytes.get(
            start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES
                ..start + shell_369_261::REFERENCE_COUNT,
        ) != Some(&[0; 3])
        || View::u32_le_at(bytes, start + shell_369_261::GUID_CODE_UNIT_COUNT)
            != Some(shell_369_261::GUID_CODE_UNIT_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::REFERENCE_COUNT)
            != Some(shell_369_261::REFERENCE_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT)
            != Some(shell_369_261::KIND_CODE_UNIT_COUNT_VALUE)
    {
        return None;
    }
    let outward = match bytes.get(start + shell_369_261::OUTWARD) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    let (guid, guid_end) = lp_utf16_bounded(
        bytes,
        start + shell_369_261::GUID_CODE_UNIT_COUNT,
        shell_369_261::GUID_CODE_UNIT_COUNT_VALUE as usize
            ..=shell_369_261::GUID_CODE_UNIT_COUNT_VALUE as usize,
    )?;
    if guid != "00000000-0000-0000-0000-000000000000"
        || guid_end != start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES
    {
        return None;
    }
    let (kind, kind_end) = lp_utf16_bounded(
        bytes,
        start + shell_369_261::KIND_CODE_UNIT_COUNT,
        shell_369_261::KIND_CODE_UNIT_COUNT_VALUE as usize
            ..=shell_369_261::KIND_CODE_UNIT_COUNT_VALUE as usize,
    )?;
    if kind != "Shell" || kind_end != start + shell_369_261::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        shell_369_261::SCALAR_REFERENCE,
        shell_369_261::GROUP_REFERENCE,
        shell_369_261::REFERENCE_ENTRY_2,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members.values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index = scope.reference_members.values().next().copied()?;
    if marked_record_reference(bytes, start + shell_369_261::SCALAR_REFERENCE)
        != Some(thickness_record_index)
    {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    if scalar.value <= 0.0 {
        return None;
    }
    Some(DesignDirectFaceOperation::Shell(crate::records::DesignShellOperation {
        thickness: scalar.value,
        thickness_record_index,
        thickness_offset: scalar.value_offset,
        outward,
        outward_offset: u64::try_from(start + shell_369_261::OUTWARD).ok()?,
    }))
}

pub(crate) fn exact_direct_face_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Shell)
        && scope.class_tag == "369"
        && scope.paired_class_tag == "261"
    {
        return exact_shell_class_369_261(bytes, records, scope);
    }
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::OffsetFaces
            if matches!(
                (
                    parameter_scope_payload_length(scope),
                    scope.reference_members.len()
                ),
                (Some(264), 4) | (Some(253), 3)
            ) && bytes.get(start + 25) == Some(&1) =>
        {
            let distance_record_index = View::u32_le_at(bytes, start + 26)?;
            if scope.reference_members.values().next_back() != Some(&distance_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, distance_record_index)?;
            Some(DesignDirectFaceOperation::OffsetFaces(crate::records::DesignOffsetFacesOperation {
                distance: scalar.value,
                distance_record_index,
                distance_offset: scalar.value_offset,
            }))
        }
        DesignFeatureFamily::Thicken if scope.reference_members.len() >= 3 => {
            if let Some(operation) = exact_legacy_thicken_class_347(bytes, records, scope) {
                return Some(operation);
            }
            let (reference_offset, thickness_is_first) = match parameter_scope_payload_length(scope)
            {
                Some(length)
                    if length
                        == 276
                            + 11 * u64::try_from(scope.reference_members.len().checked_sub(2)?)
                                .ok()?
                        && bytes.get(start + 34) == Some(&1)
                        && View::u32_le_at(bytes, start + 35)
                            == scope.reference_members.values().nth(1).copied()
                        && bytes.get(start + 39..start + 45) == Some(&[0; 6])
                        && matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46..start + 48) == Some(&[1, 1])
                        && View::u32_le_at(bytes, start + 48)
                            == scope.reference_members.values().next().copied() =>
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
            let thickness_record_index = View::u32_le_at(bytes, start + reference_offset + 1)?;
            let expected_thickness = if thickness_is_first {
                scope.reference_members.values().next()
            } else {
                scope.reference_members.values().next_back()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
            if scalar.value == 0.0 {
                return None;
            }
            Some(DesignDirectFaceOperation::Thicken(crate::records::DesignThickenOperation {
                signed_thickness: scalar.value,
                thickness_record_index,
                thickness_offset: scalar.value_offset,
            }))
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
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && View::u32_le_at(bytes, start + 56)
                                == scope.reference_members.values().nth(1).copied()
                            && bytes.get(start + 60..start + 66) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 28)?,
                            true,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    Some(268)
                        if matches!(bytes.get(start + 25), Some(0 | 1))
                            && bytes.get(start + 26) == Some(&0)
                            && bytes.get(start + 27) == Some(&1)
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && View::u32_le_at(bytes, start + 56)
                                == scope.reference_members.values().next().copied() =>
                    {
                        (
                            View::u32_le_at(bytes, start + 28)?,
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
                            && View::u32_le_at(bytes, start + 42) == Some(1)
                            && bytes.get(start + 46) == Some(&1)
                            && View::u32_le_at(bytes, start + 47)
                                == scope.reference_members.values().next().copied()
                            && bytes.get(start + 51..start + 57) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 23)?,
                            false,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    _ => return None,
                };
            let expected_thickness = if thickness_is_first {
                scope.reference_members.values().next()
            } else {
                scope.reference_members.values().next_back()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
            if scalar.value <= 0.0 {
                return None;
            }
            Some(DesignDirectFaceOperation::Shell(crate::records::DesignShellOperation {
                thickness: scalar.value,
                thickness_record_index,
                thickness_offset: scalar.value_offset,
                outward,
                outward_offset: u64::try_from(outward_offset).ok()?,
            }))
        }
        _ => None,
    }
}

pub(crate) fn exact_move_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignMoveOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Move) {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in scope.reference_members.values() {
        for (start, paired) in records.frames(*record_index) {
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            let frame_length = paired.checked_sub(start)?;
            if View::u32_le_at(bytes, after_tag) != Some(*record_index)
                || bytes.get(start + 11..start + 43) != Some(&[0; 32])
            {
                continue;
            }
            let Some((form_offset, transform_offset)) =
                move_transform_layout(&class_tag, frame_length)
            else {
                continue;
            };
            let expected_paired_class = match class_tag.as_str() {
                "447" => Some("263"),
                "456" => Some("258"),
                _ => None,
            };
            if expected_paired_class.is_some_and(|expected| {
                lp_ascii_filtered(bytes, paired, 3..=3, u8::is_ascii_digit)
                    .is_none_or(|(paired_class_tag, _)| paired_class_tag != expected)
            }) {
                continue;
            }
            if bytes.get(start + 47) != Some(&0) {
                continue;
            }
            let Ok(form) = crate::records::DesignMoveForm::try_from(View::u32_le_at(bytes, start + form_offset)?) else {
                continue;
            };
            let transform: [[f64; 4]; 4] = f64s_at(bytes, start + transform_offset, 16)?
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
                transform_offset: (start + transform_offset) as u64,
                transform_record_index: *record_index,
                form,
                form_offset: (start + form_offset) as u64,
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

/// Return the fixed envelope offsets admitted for one Move transform class.
///
/// The legacy classes carry the same matrix envelope as the current classes;
/// their class tags are the generation discriminator. Keeping the admission
/// keyed by class avoids treating an arbitrary 253-byte record as a transform.
fn move_transform_layout(class_tag: &str, frame_length: usize) -> Option<(usize, usize)> {
    let admitted = match class_tag {
        "296" | "362" | "433" | "447" if frame_length == 253 => true,
        "349" if matches!(frame_length, 254 | 274) => true,
        "368" if frame_length == 254 => true,
        "293" | "393" | "442" | "451" | "456" if frame_length == 253 => true,
        _ => false,
    };
    admitted.then_some((43, 48))
}

pub(crate) fn exact_scale_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignScaleOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Scale) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let (body_group_record_index, center_record_index, uniform_factor_offset, center) =
        if parameter_scope_payload_length(scope) == Some(303) && scope.reference_members.len() == 5
        {
            let Some([factor_record_index, body_group_record_index, _, _, center_record_index]) = scope.reference_members.values_array()
            else {
                return None;
            };
            if View::u32_le_at(bytes, start + 20)? != 1
                || bytes.get(start + 24) != Some(&0)
                || marked_record_reference(bytes, start + 33)? != *center_record_index
                || marked_record_reference(bytes, start + 44)? != *factor_record_index
                || View::u32_le_at(bytes, start + 55)? != 1
                || bytes.get(start + 59) != Some(&0)
                || View::u32_le_at(bytes, start + 60)? != 1
                || View::u32_le_at(bytes, start + 64)? != 1
                || marked_record_reference(bytes, start + 68)? != *body_group_record_index
            {
                return None;
            }
            let center = exact_point_data_construction(
                bytes,
                records,
                std::slice::from_ref(center_record_index),
                stream_types,
            )
            .map(|point| (point.position, point.position_offset));
            (
                *body_group_record_index,
                *center_record_index,
                start + 25,
                center,
            )
        } else if scope.kind() == crate::records::DesignFeatureKind::Scale
            && matches!(scope.reference_members.len(), 5 | 6)
            && scope.frame_length
                == 307 + u64::try_from(scope.reference_members.len().saturating_sub(5)).ok()? * 11
        {
            let mut references = scope.reference_members.values();
            let factor_record_index = references.next()?;
            let body_group_record_index = references.next()?;
            let center_record_index = references.next_back()?;
            if bytes.get(start + 16..start + 21)? != [0; 5]
                || marked_record_reference(bytes, start + 29)? != *center_record_index
                || marked_record_reference(bytes, start + 40)? != *factor_record_index
                || View::u32_le_at(bytes, start + 51)? != 1
                || bytes.get(start + 55) != Some(&0)
                || View::u32_le_at(bytes, start + 56)? != 1
                || View::u32_le_at(bytes, start + 60)? != 1
                || marked_record_reference(bytes, start + 64)? != *body_group_record_index
            {
                return None;
            }
            let point = exact_point_data_construction(
                bytes,
                records,
                std::slice::from_ref(center_record_index),
                stream_types,
            )?;
            (
                *body_group_record_index,
                *center_record_index,
                start + 21,
                Some((point.position, point.position_offset)),
            )
        } else {
            return None;
        };
    let uniform_factor = View::f64_le_at(bytes, uniform_factor_offset)?;
    if !uniform_factor.is_finite() || uniform_factor <= 0.0 {
        return None;
    }
    Some(DesignScaleOperation {
        body_group_record_index,
        center_record_index,
        center_position: center.map(|(value, offset)| crate::records::Located { value, offset }),
        uniform_factor,
        uniform_factor_offset: uniform_factor_offset as u64,
    })
}

pub(crate) fn exact_fixed_extrude_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::DesignParameterOwner],
) -> Option<DesignFixedExtrudeParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Extrude)
        || scope
            .extrude_prologue()
            .and_then(DesignExtrudePrologue::extent)
            != Some(DesignExtrudeExtent::OneSidedDistance)
    {
        return None;
    }
    let fixed_lanes = scope
        .reference_members
        .values()
        .filter_map(|record_index| {
            let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
            (scalar.owner_record_index == Some(scope.record_index))
                .then_some((*record_index, scalar))
        })
        .collect::<Vec<_>>();
    let embedded_distances = scope
        .reference_members
        .values()
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
        let source_kind = parameter_owners
            .iter()
            .find(|owner| {
                native_stream(&owner.id) == native_stream(&scope.id)
                    && owner.scope_record_index == scope.record_index
                    && owner.record_index == record_index
            })
            .and_then(|owner| {
                parameters
                    .iter()
                    .find(|parameter| {
                        native_stream(&parameter.id) == native_stream(&scope.id)
                            && parameter.record_index == owner.parameter_record_index
                    })
                    .map(|parameter| parameter.source_kind())
            });
        match source_kind {
            Some("AlongDistance") if lane.value != 0.0 && along_distance.is_none() => {
                along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(scalar));
            }
            Some("TaperAngle") if taper_angle.is_none() => taper_angle = Some(scalar),
            Some("AlongDistance") if along_distance.is_some() && lane.value == 0.0 => {}
            Some(_) => return None,
            None => match lane.ordinal {
                0 if lane.value != 0.0 && along_distance.is_none() => {
                    along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(scalar));
                }
                0 if along_distance.is_some() && lane.value == 0.0 => {}
                1 if taper_angle.is_none() => taper_angle = Some(scalar),
                _ => return None,
            },
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
                || View::u32_le_at(bytes, start + 32)? != 1
                || marked_record_reference(bytes, start + 36).is_none()
                || bytes.get(start + 41..start + 47) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 47)? != 210
                || View::u32_le_at(bytes, start + 59)? != 210
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
            let value = View::f64_le_at(bytes, start + 51)?;
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
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Fillet) {
        return None;
    }
    let lanes = scope
        .reference_members
        .values()
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
    use crate::records::{DesignFixedFilletIntermediate, DesignFixedFilletLaw, DesignFixedFilletScalar};
    let scalar = |(record_index, scalar): &(u32, FixedScalarFrame)| DesignFixedFilletScalar {
        value: scalar.value, record_index: *record_index, value_offset: scalar.value_offset,
    };
    let group = |tangency_lane: Option<&(u32, FixedScalarFrame)>, law: DesignFixedFilletLaw| {
        let tangency_weight = tangency_lane.map(scalar);
        if tangency_weight.as_ref().is_some_and(|weight| weight.value <= 0.0)
            || law.radii().any(|radius| radius.value < 0.0)
            || law.radii().all(|radius| radius.value == 0.0)
            || law.intermediate().iter().any(|row| !(0.0..1.0).contains(&row.parameter.value))
            || law.intermediate().windows(2).any(|pair| pair[0].parameter.value >= pair[1].parameter.value)
        {
            return None;
        }
        Some(DesignFixedFilletGroup { tangency_weight, law })
    };
    let groups = if lanes.len() == 1 {
        vec![group(None, DesignFixedFilletLaw::Constant(scalar(&lanes[0])))?]
    } else if lanes.len() % 2 == 0 {
        lanes.chunks_exact(2).map(|pair| group(Some(&pair[0]), DesignFixedFilletLaw::Constant(scalar(&pair[1]))))
            .collect::<Option<Vec<_>>>()?
    } else {
        vec![group(Some(&lanes[0]), DesignFixedFilletLaw::Variable {
            start: scalar(&lanes[1]), end: scalar(&lanes[2]),
            intermediate: lanes[3..].chunks_exact(2).map(|pair| DesignFixedFilletIntermediate {
                radius: scalar(&pair[0]), parameter: scalar(&pair[1]),
            }).collect(),
        })?]
    };
    Some(DesignFixedFilletParameters { groups })
}

pub(crate) fn exact_fixed_chamfer_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFixedChamferParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Chamfer) {
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
        .values()
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

fn unique_revolve_angle_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: Option<u32>,
) -> Option<&'a DesignParameterOwner> {
    let mut candidates = parameter_owners.iter().filter(|owner| {
        native_stream(&owner.id) == native_stream(&scope.id)
            && owner.scope_record_index == scope.record_index
            && scope.reference_members.values().any(|value| value == &owner.record_index)
            && record_index.is_none_or(|index| owner.record_index == index)
            && owner.local_ordinal == 0
            && owner.evaluated_value.is_finite()
            && owner.evaluated_value > 0.0
    });
    let angle = candidates.next()?;
    candidates.next().is_none().then_some(angle)
}

pub(crate) fn exact_path_feature_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignPathFeatureConstruction> {
    let start = usize::try_from(scope.byte_offset).ok()?;
    let operation = |offset| {
        Some(match View::u32_le_at(bytes, offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        })
    };
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::Revolve
            if matches!(scope.reference_members.len(), 6 | 8)
                && bytes.get(start + revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
                && View::u32_le_at(bytes, start + revolve::STRUCTURAL_CONSTANT) == Some(1) =>
        {
            let angle = unique_revolve_angle_owner(scope, parameter_owners, None)?;
            Some(DesignPathFeatureConstruction::Revolve(crate::records::DesignRevolveConstruction {
                operation: operation(start + revolve::OPERATION)?,
                operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                angle: angle.evaluated_value,
                angle_record_index: angle.record_index,
                angle_offset: angle.evaluated_value_offset,
                opposite_angle: None,
            }))
        }
        DesignFeatureFamily::Revolve
            if parameter_scope_payload_length(scope) == Some(372)
                && scope.reference_members.len() == 7
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0) =>
        {
            let lanes = scope
                .reference_members
                .values()
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
            Some(DesignPathFeatureConstruction::Revolve(crate::records::DesignRevolveConstruction {
                operation: operation(start + revolve::OPERATION)?,
                operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                angle: angle.value,
                angle_record_index: *angle_record_index,
                angle_offset: angle.value_offset,
                opposite_angle: Some(crate::records::Located { value: *opposite_angle_record_index, offset: opposite.value_offset }),
            }))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag == "407"
                && scope.paired_class_tag == "258"
                && parameter_scope_payload_length(scope) == Some(363)
                && scope.reference_members.len() == 8
                && View::u32_le_at(bytes, start + 25) == Some(2)
                && bytes.get(start + 29) == Some(&0)
                && View::u32_le_at(bytes, start + 30) == Some(1)
                && bytes.get(start + 34) == Some(&1)
                && bytes.get(start + 43..start + 45) == Some(&[0; 2]) =>
        {
            let angle_record_index = u32::try_from(View::u64_le_at(bytes, start + 35)?).ok()?;
            if scope.reference_members.values().nth(6) != Some(&angle_record_index) {
                return None;
            }
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index))?;
            Some(DesignPathFeatureConstruction::Revolve(crate::records::DesignRevolveConstruction {
                operation: operation(start + 21)?,
                operation_offset: u64::try_from(start + 21).ok()?,
                angle: angle.evaluated_value,
                angle_record_index,
                angle_offset: angle.evaluated_value_offset,
                opposite_angle: None,
            }))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag == "403"
                && scope.paired_class_tag == "258"
                && scope.frame_length == 387
                && scope.reference_members.len() == 8
                && View::u32_le_at(bytes, start + class_403_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + class_403_revolve::DIRECTION_KIND..start + 31)
                    == Some(&[0, 1]) =>
        {
            let angle_record_index =
                marked_record_reference(bytes, start + class_403_revolve::ANGLE_REFERENCE_MARKER)?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index))?;
            Some(DesignPathFeatureConstruction::Revolve(crate::records::DesignRevolveConstruction {
                operation: operation(start + class_403_revolve::OPERATION)?,
                operation_offset: u64::try_from(start + class_403_revolve::OPERATION).ok()?,
                angle: angle.evaluated_value,
                angle_record_index,
                angle_offset: angle.evaluated_value_offset,
                opposite_angle: None,
            }))
        }
        DesignFeatureFamily::Loft
            if scope.class_tag.len() == 3
                && bytes
                    .get(start + compact_loft::ZERO_RUN_10..start + compact_loft::ONE_RUN_4)
                    == Some(&[0; 10])
                && bytes.get(start + compact_loft::ONE_RUN_4..start + compact_loft::OPERATION)
                    == Some(&[1; 4])
                && bytes.get(start + compact_loft::ZERO_FLAG) == Some(&0)
                && View::u32_le_at(bytes, start + compact_loft::ALL_ONES) == Some(u32::MAX)
                && bytes.get(start + compact_loft::ZERO_RUN_11..start + compact_loft::LEN)
                    == Some(&[0; 11]) =>
        {
            Some(DesignPathFeatureConstruction::Loft(crate::records::DesignLoftConstruction {
                operation: operation(start + compact_loft::OPERATION)?,
                operation_offset: u64::try_from(start + compact_loft::OPERATION).ok()?,
            }))
        }
        DesignFeatureFamily::Loft
            if scope.class_tag.len() == 3
                && parameter_scope_payload_length(scope).is_some_and(|length| length >= 368) =>
        {
            Some(DesignPathFeatureConstruction::Loft(crate::records::DesignLoftConstruction {
                operation: operation(start + 29)?,
                operation_offset: u64::try_from(start + 29).ok()?,
            }))
        }
        DesignFeatureFamily::Sweep => {
            let lanes = scope
                .reference_members
                .values()
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
            Some(DesignPathFeatureConstruction::Sweep(crate::records::DesignSweepConstruction {
                operation: operation(start + 25)?,
                operation_offset: u64::try_from(start + 25).ok()?,
                values: lanes.map(|(_, scalar)| scalar.value),
                record_indexes: lanes.map(|(record_index, _)| record_index),
                value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
            }))
        }
        DesignFeatureFamily::Pipe => {
            let legacy_prefix_layout = matches!(
                (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
                ("405", "259") | ("475", "260")
            );
            let owner_layout = matches!(
                (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
                ("421", "257")
            );
            if legacy_prefix_layout
                && (bytes.get(start + legacy_pipe::ZERO_RUN_9..start + legacy_pipe::PREFIX_MARKER)
                    != Some(&[0; 9])
                    || bytes.get(start + legacy_pipe::PREFIX_MARKER)
                        != Some(&legacy_pipe::PREFIX_MARKER_VALUE)
                    || bytes.get(start + legacy_pipe::ZERO_RUN_5..start + legacy_pipe::OPERATION)
                        != Some(&[0; 5]))
            {
                return None;
            }
            let lanes = if owner_layout {
                exact_pipe_owner_lanes(scope, parameter_owners)?.to_vec()
            } else {
                scope
                    .reference_members
                    .values()
                    .filter_map(|record_index| {
                        let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                        (scalar.owner_record_index == Some(scope.record_index))
                            .then_some((*record_index, scalar))
                    })
                    .collect::<Vec<_>>()
            };
            let lanes: [(u32, FixedScalarFrame); 4] = lanes.try_into().ok()?;
            if lanes
                .iter()
                .enumerate()
                .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
            {
                return None;
            }
            let (operation_offset, section_shape_offset, filled_offset) = if legacy_prefix_layout {
                (
                    start + legacy_pipe::OPERATION,
                    start + legacy_pipe::SECTION_SHAPE,
                    start + legacy_pipe::FILLED,
                )
            } else {
                (
                    start + fixed_pipe::OPERATION,
                    start + fixed_pipe::SECTION_SHAPE,
                    start + fixed_pipe::FILLED,
                )
            };
            let section_shape = *bytes.get(section_shape_offset)?;
            let filled = match *bytes.get(filled_offset)? {
                0 => false,
                1 => true,
                _ => return None,
            };
            Some(DesignPathFeatureConstruction::Pipe(crate::records::DesignPipeConstruction {
                operation: operation(operation_offset)?,
                operation_offset: u64::try_from(operation_offset).ok()?,
                section_shape: crate::records::DesignPipeSectionShape::from_code(section_shape),
                section_shape_offset: u64::try_from(section_shape_offset).ok()?,
                filled,
                filled_offset: u64::try_from(filled_offset).ok()?,
                values: lanes.map(|(_, scalar)| scalar.value),
                record_indexes: lanes.map(|(record_index, _)| record_index),
                value_offsets: lanes.map(|(_, scalar)| scalar.value_offset),
            }))
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
    for record_index in scope.reference_members.values() {
        for (start, paired) in records.frames(*record_index) {
            let frame_length = paired.checked_sub(start)?;
            let (matrix_at, reference) = match frame_length {
                work_plane_legacy::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"400")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_legacy::MATRIX)
                            == Some(&[0u8; work_plane_legacy::MATRIX - 11][..]) =>
                {
                    (start + work_plane_legacy::MATRIX, None)
                }
                work_plane_class_290::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"290")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_class_290::PREFIX_MARKER)
                            == Some(&[0u8; work_plane_class_290::PREFIX_MARKER - 11][..])
                        && bytes.get(
                            start + work_plane_class_290::PREFIX_MARKER
                                ..start + work_plane_class_290::MATRIX,
                        ) == Some(&[1, 1, 0, 0][..]) =>
                {
                    (start + work_plane_class_290::MATRIX, None)
                }
                work_plane_325::LEN
                    if matches!(
                        (
                            bytes.get(start + 4..start + 7),
                            bytes.get(paired + 4..paired + 7),
                        ),
                        (Some(b"320"), Some(b"258"))
                            | (Some(b"380"), Some(b"262"))
                            | (Some(b"308" | b"431"), Some(b"257"))
                            | (Some(b"364"), Some(b"263"))
                    ) && bytes.get(start + 11..start + work_plane_325::MATRIX)
                        == Some(&[0u8; work_plane_325::MATRIX - 11][..]) =>
                {
                    (start + work_plane_325::MATRIX, None)
                }
                work_plane_class_256::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"256")
                        && bytes.get(paired + 4..paired + 7) == Some(b"262")
                        && bytes.get(start + 11..start + work_plane_class_256::OPAQUE_U16)
                            == Some(&[0u8; work_plane_class_256::OPAQUE_U16 - 11][..])
                        && bytes.get(
                            start + work_plane_class_256::ZERO_PAIR
                                ..start + work_plane_class_256::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_class_256::MATRIX - work_plane_class_256::ZERO_PAIR]
                                [..],
                        ) =>
                {
                    (start + work_plane_class_256::MATRIX, None)
                }
                work_plane_class_337_325::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"337")
                        && bytes.get(paired + 4..paired + 7) == Some(b"266")
                        && bytes.get(start + 11..start + work_plane_class_337_325::OPAQUE_U16)
                            == Some(&[0u8; work_plane_class_337_325::OPAQUE_U16 - 11][..])
                        && bytes.get(
                            start + work_plane_class_337_325::ZERO_PAIR
                                ..start + work_plane_class_337_325::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_class_337_325::MATRIX
                                - work_plane_class_337_325::ZERO_PAIR][..],
                        ) =>
                {
                    (start + work_plane_class_337_325::MATRIX, None)
                }
                work_plane_class_322_332::LEN
                    if bytes.get(start + 4..start + 7) == Some(b"322")
                        && bytes.get(paired + 4..paired + 7) == Some(b"261")
                        && bytes.get(start + 11..start + work_plane_class_322_332::MATRIX)
                            == Some(&[0u8; work_plane_class_322_332::MATRIX - 11][..]) =>
                {
                    (start + work_plane_class_322_332::MATRIX, None)
                }
                321 if bytes.get(start + 11..start + 49) == Some(&[0u8; 38][..]) => {
                    (start + 49, None)
                }
                work_plane_321_opaque::LEN
                    if matches!(
                        (
                            bytes.get(start + 4..start + 7),
                            bytes.get(paired + 4..paired + 7),
                        ),
                        (Some(b"341"), Some(b"261")) | (Some(b"346"), Some(b"262"))
                    ) && bytes.get(start + 11..start + work_plane_321_opaque::OPAQUE_U16)
                        == Some(&[0u8; work_plane_321_opaque::OPAQUE_U16 - 11][..])
                        && bytes.get(
                            start + work_plane_321_opaque::ZERO_PAIR
                                ..start + work_plane_321_opaque::MATRIX,
                        ) == Some(
                            &[0u8; work_plane_321_opaque::MATRIX
                                - work_plane_321_opaque::ZERO_PAIR][..],
                        ) =>
                {
                    (start + work_plane_321_opaque::MATRIX, None)
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
                    (Some(b"279"), Some(b"266"))
                        | (Some(b"409"), Some(b"258"))
                        | (Some(b"450"), Some(b"259"))
                ) && bytes.get(start + 11..start + 50) == Some(&[0u8; 39][..]) =>
                {
                    (start + 50, None)
                }
                work_plane_337::LEN
                    if matches!(
                        (
                            bytes.get(start + 4..start + 7),
                            bytes.get(paired + 4..paired + 7),
                        ),
                        (Some(b"350" | b"409"), Some(b"258"))
                    ) && bytes.get(start + 11..start + work_plane_337::MATRIX)
                        == Some(&[0u8; work_plane_337::MATRIX - 11][..]) =>
                {
                    (start + work_plane_337::MATRIX, None)
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
                        Some((View::u32_le_at(bytes, start + 58)?, (start + 58) as u64)),
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
    if scope.kind() != crate::records::DesignFeatureKind::WorkAxis {
        return None;
    }
    exact_two_point_work_axis_construction(bytes, records, scope)
        .or_else(|| exact_direct_work_axis_construction(bytes, records, scope))
}

fn exact_two_point_work_axis_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignWorkAxisConstruction> {
    let Some([axis_record_index, _, first_point_record_index, _, second_point_record_index]) = scope.reference_members.values_array()
    else {
        return None;
    };
    let axis_frames = records.frames(*axis_record_index).collect::<Vec<_>>();
    let [(axis_start, axis_paired)] = axis_frames.as_slice() else {
        return None;
    };
    if axis_paired.checked_sub(*axis_start)? != 232
        || bytes.get(axis_start + 11..axis_start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, axis_start + 21)? != 8
        || View::u32_le_at(bytes, axis_start + 118)? != 2
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
            || View::u32_le_at(bytes, reference_at + 1)? != *expected
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
        source: Some(DesignWorkAxisSource::TwoPoint {
            point_record_indices,
            point_offsets,
        }),
    })
}

fn exact_direct_work_axis_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignWorkAxisConstruction> {
    let Some([carrier_record_index, support_record_index]) = scope.reference_members.values_array() else {
        return None;
    };
    let (
        carrier_class,
        carrier_paired_class,
        carrier_length,
        support_class,
        support_paired_class,
        value_count_offset,
        axis_values_offset,
        reference_count_offset,
        reference_preamble_offset,
    ) = match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length,
    ) {
        ("302", "262", 268) => (
            "297",
            "262",
            work_axis_297::LEN,
            "306",
            "262",
            work_axis_297::VALUE_COUNT,
            work_axis_297::AXIS_VALUES,
            work_axis_297::REFERENCE_COUNT,
            work_axis_297::REFERENCE_PREAMBLE,
        ),
        ("361", "258", 254) => (
            "335",
            "258",
            work_axis_335::LEN,
            "349",
            "258",
            work_axis_335::VALUE_COUNT,
            work_axis_335::AXIS_VALUES,
            work_axis_335::REFERENCE_COUNT,
            work_axis_335::REFERENCE_PREAMBLE,
        ),
        _ => return None,
    };
    let carrier_frames = records.frames(*carrier_record_index).collect::<Vec<_>>();
    let [(carrier_start, carrier_paired)] = carrier_frames.as_slice() else {
        return None;
    };
    let carrier_primary_class =
        exact_indexed_header_at(bytes, *carrier_start, *carrier_record_index)?;
    let carrier_paired_class_tag =
        exact_indexed_header_at(bytes, *carrier_paired, *carrier_record_index)?;
    if carrier_paired.checked_sub(*carrier_start)? != carrier_length
        || carrier_primary_class != carrier_class
        || carrier_paired_class_tag != carrier_paired_class
    {
        return None;
    }
    let support_frames = records.frames(*support_record_index).collect::<Vec<_>>();
    let [(support_start, support_paired)] = support_frames.as_slice() else {
        return None;
    };
    let support_primary_class =
        exact_indexed_header_at(bytes, *support_start, *support_record_index)?;
    let support_paired_class_tag =
        exact_indexed_header_at(bytes, *support_paired, *support_record_index)?;
    if support_paired.checked_sub(*support_start)? != 293
        || support_primary_class != support_class
        || support_paired_class_tag != support_paired_class
    {
        return None;
    }
    if bytes.get(*carrier_start + 11..*carrier_start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, *carrier_start + value_count_offset)? != 8
        || View::u32_le_at(bytes, *carrier_start + reference_count_offset)? != 6
        || View::u32_le_at(bytes, *carrier_start + reference_preamble_offset)? != 1
    {
        return None;
    }
    let values = f64s_at(bytes, (*carrier_start).checked_add(axis_values_offset)?, 8)?;
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
    Some(DesignWorkAxisConstruction {
        origin,
        displacement,
        origin_offset: u64::try_from((*carrier_start).checked_add(axis_values_offset)?).ok()?,
        displacement_offset: u64::try_from(
            (*carrier_start).checked_add(axis_values_offset + 3 * 8)?,
        )
        .ok()?,
        source: Some(DesignWorkAxisSource::DirectCarrier {
            carrier_record_index: *carrier_record_index,
            support_record_index: *support_record_index,
        }),
    })
}

pub(crate) fn exact_joint_origin_frame(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<ScopePlacementFrame> {
    if scope.kind() != crate::records::DesignFeatureKind::JointOrigin
        || matches!(scope.frame_length, 300 | 322 | 344)
    {
        return None;
    }
    let mut candidates = Vec::new();
    for record_index in scope.reference_members.values() {
        for (start, paired) in records.frames(*record_index) {
            if paired.checked_sub(start)? == joint_origin_class_337_266::LEN
                && bytes.get(start + 4..start + 7) == Some(b"337")
                && bytes.get(paired + 4..paired + 7) == Some(b"266")
                && bytes.get(start + 11..start + joint_origin_class_337_266::MATRIX_PREFIX)
                    == Some(&[0; joint_origin_class_337_266::MATRIX_PREFIX - 11][..])
                && bytes.get(
                    start + joint_origin_class_337_266::MATRIX_PREFIX
                        ..start + joint_origin_class_337_266::MATRIX,
                ) == Some(&joint_origin_class_337_266::MATRIX_PREFIX_VALUE)
            {
                let values = f64s_at(bytes, start + joint_origin_class_337_266::MATRIX, 16)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.into_iter().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if valid_sketch_transform(&transform) {
                    candidates.push(ScopePlacementFrame {
                        transform,
                        transform_offset: (start + joint_origin_class_337_266::MATRIX) as u64,
                        reference: None,
                    });
                }
                continue;
            }
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
            let count = View::u32_le_at(bytes, cursor)?;
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct PointDataLevel {
    /// Byte offset of `point3d`'s first coordinate.
    position_at: usize,
    /// Construction rule that produced the point.
    reference_type: u32,
    /// Byte offset of the serialized construction rule.
    reference_type_at: usize,
    /// Counted input-reference run that closes the level.
    inputs: Vec<DesignWorkPointInput>,
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
    let reference_type_at = cursor;
    let reference_type = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    if version >= 3 {
        cursor = cursor.checked_add(24)?;
    }
    let arity = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
    cursor = cursor.checked_add(4)?;
    if arity == 0 || arity > end.checked_sub(cursor)? {
        return None;
    }
    let mut inputs = Vec::with_capacity(arity);
    for _ in 0..arity {
        let reference_offset = cursor.checked_add(1)?;
        let reference = take_reference(body, &mut cursor)?;
        inputs.push(DesignWorkPointInput {
            record_index: u32::try_from(reference.target?).ok()?,
            reference_offset: u64::try_from(reference_offset).ok()?,
            carrier: None,
        });
    }
    Some(PointDataLevel {
        position_at,
        reference_type,
        reference_type_at,
        inputs,
    })
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
pub(crate) fn exact_work_point_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignWorkPointConstruction> {
    if scope.kind() != crate::records::DesignFeatureKind::WorkPoint {
        return None;
    }
    exact_point_data_construction(bytes, records, scope.reference_members.values(), stream_types)
}

pub(crate) fn exact_point_data_construction<'a>(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    point_record_indices: impl IntoIterator<Item = &'a u32>,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignWorkPointConstruction> {
    let mut candidates = Vec::new();
    for record_index in point_record_indices {
        for (start, paired) in records.frames(*record_index) {
            let Some((_class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if View::u32_le_at(bytes, after_tag) != Some(*record_index) {
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
            let levels = stored
                .map_or_else(|| (0..=3).collect::<Vec<_>>(), |(_, version)| vec![version])
                .into_iter()
                .filter_map(|version| point_data_level(bytes, payload_at, paired, version))
                .fold(Vec::new(), |mut levels, level| {
                    if !levels.contains(&level) {
                        levels.push(level);
                    }
                    levels
                });
            // Agreement is over the levels the fitting versions name, so the
            // duplicates are removed by value regardless of version order.
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
                candidates.push(DesignWorkPointConstruction {
                    point_record_index: *record_index,
                    point_record_byte_offset: u64::try_from(start).ok()?,
                    position,
                    position_offset: u64::try_from(level.position_at).ok()?,
                    rule: DesignWorkPointRule::from_serialized(
                        level.reference_type,
                        level.inputs.clone(),
                    ).ok()?,
                    reference_type_offset: u64::try_from(level.reference_type_at).ok()?,
                });
            }
        }
    }
    if candidates.len() != 1 {
        return None;
    }
    candidates.pop()
}

/// Type GUID of the point-and-direction carrier selected by a `Hole` scope.
const HOLE_POINT_DATA_TYPE_GUID: &str = "F2A7590D-6654-4674-B393-A2AEF4FEC48A";

/// Type GUID of the direct persistent face selection carried by a `Hole`.
const HOLE_FACE_SELECTION_TYPE_GUID: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";

/// Accepted norm error for a serialized Hole drilling direction.
const EPS_HOLE_DIRECTION_NORM: f64 = 1.0e-12;

/// Decode the exact point-and-direction carrier owned by a `Hole` scope.
///
/// The carrier's versioned base level is distinct from the `WorkPoint` level:
/// it writes the position, direction, two construction parameters, `refType`,
/// and a counted input-reference run. Version four inserts tangent-point data
/// before that run; version one omits it and retains additional class members
/// before the paired header. The type GUID and version select the layout; the
/// dynamic class tag does not.
pub(crate) fn exact_hole_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignHoleConstruction> {
    if scope.kind() != crate::records::DesignFeatureKind::Hole {
        return None;
    }
    let face_selection = exact_hole_face_selection(bytes, records, scope, stream_types);
    let mut candidates = Vec::new();
    for record_index in scope.reference_members.values() {
        let Some((type_guid, version)) = stream_types.get(&u64::from(*record_index)) else {
            continue;
        };
        if *type_guid != HOLE_POINT_DATA_TYPE_GUID || !matches!(*version, 1 | 4) {
            continue;
        }
        for (start, paired_at) in records.frames(*record_index) {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || after_tag != start + 7
                || View::u32_le_at(bytes, after_tag) != Some(*record_index)
            {
                continue;
            }
            let Some((_name, payload_at)) =
                lp_ascii_filtered(bytes, after_tag + 8, 0..=256, u8::is_ascii_graphic)
            else {
                continue;
            };
            if let Some(candidate) = hole_construction_frame_at(
                bytes,
                start,
                paired_at,
                payload_at,
                *record_index,
                *version,
                face_selection.clone(),
            ) {
                candidates.push(candidate);
            }
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn exact_hole_face_selection(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignHoleFaceSelection> {
    let mut candidates = Vec::new();
    for record_index in scope.reference_members.values() {
        if stream_types.get(&u64::from(*record_index)) != Some(&(HOLE_FACE_SELECTION_TYPE_GUID, 1))
        {
            continue;
        }
        for (start, _paired_at) in records.frames(*record_index) {
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || after_tag != start + 7
                || View::u32_le_at(bytes, after_tag) != Some(*record_index)
            {
                continue;
            }
            let Some(frame) = parse_entity_selection_frame(
                bytes,
                *record_index,
                u64::try_from(start).ok()?,
                &class_tag,
            ) else {
                continue;
            };
            candidates.push(DesignHoleFaceSelection {
                record_index: frame.record_index,
                byte_offset: frame.byte_offset,
                class_tag: frame.class_tag,
                asset_id: frame.asset_id,
                asset_id_offset: frame.asset_id_offset,
                context_id: frame.context_id,
                context_id_offset: frame.context_id_offset,
                identity_record_index: frame.identity_record_index,
                identity_record_offset: frame.identity_record_offset,
                primary_identity: frame.primary_identity,
                primary_identity_offset: frame.primary_identity_offset,
                secondary: frame.secondary,
                historical_face_candidates: Vec::new(),
                next_record_index: frame.next_record_index,
                next_byte_offset: frame.next_byte_offset,
            });
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn hole_construction_frame_at(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    payload_at: usize,
    point_record_index: u32,
    version: u32,
    face_selection: Option<DesignHoleFaceSelection>,
) -> Option<DesignHoleConstruction> {
    let body = bytes.get(..paired_at)?;
    let mut cursor = payload_prologue(body, payload_at, paired_at)?;
    let _bounding_box_index = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let position_at = cursor;
    cursor = cursor.checked_add(24)?;
    let direction_at = cursor;
    cursor = cursor.checked_add(24)?;
    let point_parameters_at = cursor;
    cursor = cursor.checked_add(16)?;
    let reference_type_at = cursor;
    let reference_type = View::u32_le_at(body, cursor)?;
    cursor = cursor.checked_add(4)?;
    let tangent_point_data = if version == 4
    {
        let prefix = *body.get(cursor)?;
        cursor = cursor.checked_add(1)?;
        let tangent_point_data_at = cursor;
        cursor = cursor.checked_add(24)?;
        let tangent_point_data: [f64; 3] =
            f64s_at(body, tangent_point_data_at, 3)?.try_into().ok()?;
        Some(crate::records::DesignHoleTangentPoint {
            prefix,
            data: crate::records::Located { value: tangent_point_data, offset: u64::try_from(tangent_point_data_at).ok()? },
        })
    } else if version == 1 {
        None
    } else {
        return None;
    };
    let input_count = usize::try_from(View::u32_le_at(body, cursor)?).ok()?;
    cursor = cursor.checked_add(4)?;
    if input_count == 0 || input_count > paired_at.checked_sub(cursor)? {
        return None;
    }
    let position: [f64; 3] = f64s_at(body, position_at, 3)?.try_into().ok()?;
    let direction: [f64; 3] = f64s_at(body, direction_at, 3)?.try_into().ok()?;
    let point_parameters: [f64; 2] = f64s_at(body, point_parameters_at, 2)?.try_into().ok()?;
    let direction_norm = direction
        .iter()
        .map(|component| component * component)
        .sum::<f64>();
    if position
        .iter()
        .chain(direction.iter())
        .chain(point_parameters.iter())
        .chain(tangent_point_data.iter().flat_map(|tangent| tangent.data.value.iter()))
        .any(|value| !value.is_finite())
        || (direction_norm - 1.0).abs() > EPS_HOLE_DIRECTION_NORM
    {
        return None;
    }
    let mut input_records = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        let reference_at = cursor;
        let reference = take_reference(body, &mut cursor)?;
        let target = u32::try_from(reference.target?).ok()?;
        input_records.push(crate::records::Located { value: target, offset: u64::try_from(reference_at.checked_add(1)?).ok()? });
    }
    if (version == 4 && cursor != paired_at)
        || (version == 1 && next_indexed_record_offset(bytes, cursor)? != paired_at)
    {
        return None;
    }
    Some(DesignHoleConstruction {
        point_record_index,
        point_record_byte_offset: u64::try_from(start).ok()?,
        position,
        position_offset: u64::try_from(position_at).ok()?,
        direction,
        direction_offset: u64::try_from(direction_at).ok()?,
        point_parameters,
        point_parameter_offsets: [
            u64::try_from(point_parameters_at).ok()?,
            u64::try_from(point_parameters_at.checked_add(8)?).ok()?,
        ],
        reference_type,
        reference_type_offset: u64::try_from(reference_type_at).ok()?,
        tangent_point_data,
        input_records,
        face_selection,
    })
}

pub(crate) fn exact_combine_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignCombineOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Combine)
        || scope.reference_members.len() < 4
        || !scope.reference_members.len().is_multiple_of(2)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let compact = scope.class_tag == "387"
        && scope.paired_class_tag == "258"
        && parameter_scope_payload_length(scope) == Some(314);
    let extended_reference =
        scope.class_tag == "329" && scope.paired_class_tag == "261" && scope.frame_length == 363;
    let (form, operation_offset, keep_tools_offset) = if compact {
        if bytes.get(start + combine_compact::ZERO_RUN_10..start + combine_compact::OPERATION)?
            != [0; 10]
            || bytes
                .get(start + combine_compact::ZERO_RUN_3..start + combine_compact::REFERENCE_FORM)?
                != [0; 3]
            || bytes.get(
                start + combine_compact::REFERENCE_FORM..start + combine_compact::CONSTANT_ONE,
            )? != [1, 0]
            || View::u32_le_at(bytes, start + combine_compact::CONSTANT_ONE) != Some(1)
            || bytes.get(start + combine_compact::REFERENCE_MARKER) != Some(&1)
            || View::u64_le_at(bytes, start + combine_compact::REFERENCE_VALUE) == Some(0)
            || bytes.get(start + combine_compact::REFERENCE_TAIL..start + combine_compact::LEN)?
                != [0; 2]
        {
            return None;
        }
        (
            DesignCombineForm::Compact,
            start + combine_compact::OPERATION,
            start + combine_compact::KEEP_TOOLS,
        )
    } else if extended_reference {
        let mut reference_at = start.checked_add(combine_extended::REFERENCE_MARKER)?;
        let reference = take_reference(bytes, &mut reference_at)?;
        if bytes
            .get(start + combine_extended::ZERO_RUN_18..start + combine_extended::FORM_MARKER)?
            != [0; 18]
            || bytes.get(start + combine_extended::FORM_MARKER) != Some(&1)
            || reference.target.is_none_or(|target| target == 0)
            || reference.segment.is_some()
            || reference.link_name.is_some()
            || reference_at != start.checked_add(combine_extended::LEN)?
        {
            return None;
        }
        (
            DesignCombineForm::ExtendedReference,
            start + combine_extended::OPERATION,
            start + combine_extended::KEEP_TOOLS,
        )
    } else {
        if bytes.get(start + combine_standard::ZERO_RUN_9..start + combine_standard::OPERATION)?
            != [0; 9]
            || bytes.get(start + combine_standard::ZERO_FLAG) != Some(&0)
            || bytes.get(start + combine_standard::ZERO_RUN_7..start + combine_standard::LEN)?
                != [0; 7]
        {
            return None;
        }
        (
            DesignCombineForm::Standard,
            start + combine_standard::OPERATION,
            start + combine_standard::KEEP_TOOLS,
        )
    };
    let operation = match View::u32_le_at(bytes, operation_offset)? {
        1 => cadmpeg_ir::features::BooleanKind::Join,
        2 => cadmpeg_ir::features::BooleanKind::Cut,
        3 => cadmpeg_ir::features::BooleanKind::Intersect,
        _ => return None,
    };
    let keep_tools = match bytes.get(keep_tools_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let mut target = None;
    let mut tools = Vec::with_capacity(scope.reference_members.len() / 2);
    for (operation_record_index, selection_record_index) in scope.reference_members.values().step_by(2)
        .zip(scope.reference_members.values().skip(1).step_by(2)) {
        let [operation_at, operation_end] = records.offsets(*operation_record_index) else {
            return None;
        };
        let role = combine_operation_identity_role(
            bytes.get(*operation_at..*operation_end)?,
            *selection_record_index,
        )?;
        let [selection_at, selection_end] = records.offsets(*selection_record_index) else {
            return None;
        };
        if !contains_consecutive_guid_pair(bytes.get(*selection_at..*selection_end)?) {
            return None;
        }
        match role {
            CombineOperandRole::Target => {
                if target.replace(*selection_record_index).is_some() {
                    return None;
                }
            }
            CombineOperandRole::Tool => tools.push(DesignCombineBodySelection {
                record_index: *selection_record_index,
                external_identity: exact_combine_external_body_identity(
                    bytes, *selection_at, *selection_end, scope.record_index, *selection_record_index,
                ),
            }),
        }
    }
    let target = target?;
    let mut tools = tools.into_iter();
    let tools = crate::records::DesignCombineTools { first: tools.next()?, additional: tools.collect() };
    Some(DesignCombineOperation {
        form,
        operation,
        operation_offset: u64::try_from(operation_offset).ok()?,
        keep_tools,
        keep_tools_offset: u64::try_from(keep_tools_offset).ok()?,
        target_record_index: target,
        tools,
    })
}

struct ExternalReferenceIdentity {
    target: u64,
    target_offset: u64,
    segment: u32,
    segment_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    link_name: String,
    link_name_offset: u64,
    version: Option<crate::records::DesignExternalVersion>,
}

fn take_external_reference_identity(
    bytes: &[u8],
    cursor: &mut usize,
) -> Option<ExternalReferenceIdentity> {
    if bytes.get(*cursor) != Some(&1) {
        return None;
    }
    let target_at = cursor.checked_add(1)?;
    let target = View::u64_le_at(bytes, target_at)?;
    if target == 0 || bytes.get(target_at.checked_add(8)?) != Some(&1) {
        return None;
    }
    let segment_at = target_at.checked_add(9)?;
    let segment = View::u32_le_at(bytes, segment_at)?;
    let asset_at = segment_at.checked_add(4)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_at, 1..=256)?;
    if !is_guid_relaxed(&asset_id) || bytes.get(after_asset_id) != Some(&0) {
        return None;
    }
    let link_name_at = after_asset_id.checked_add(1)?;
    let (link_name, after_link_name) = lp_utf16_bounded(bytes, link_name_at, 1..=256)?;
    let (version, end) =
        match bytes.get(after_link_name)? {
            0 => (None, after_link_name.checked_add(1)?),
            1 => {
                let property_key_at = after_link_name.checked_add(1)?;
                let (property_key, after_property_key) =
                    lp_utf16_bounded(bytes, property_key_at, 1..=256)?;
                let version_urn_at = after_property_key;
                let (version_urn, end) = lp_utf16_bounded(bytes, version_urn_at, 1..=256)?;
                if !is_guid_relaxed(&property_key) {
                    return None;
                }
                (
                    Some(crate::records::DesignExternalVersion {
                        property_key: crate::records::Located { value: property_key, offset: u64::try_from(property_key_at.checked_add(4)?).ok()? },
                        version_urn: crate::records::Located { value: version_urn, offset: u64::try_from(version_urn_at.checked_add(4)?).ok()? },
                    }),
                    end,
                )
            }
            _ => return None,
        };
    *cursor = end;
    Some(ExternalReferenceIdentity {
        target,
        target_offset: u64::try_from(target_at).ok()?,
        segment,
        segment_offset: u64::try_from(segment_at).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(asset_at.checked_add(4)?).ok()?,
        link_name,
        link_name_offset: u64::try_from(link_name_at.checked_add(4)?).ok()?,
        version,
    })
}

fn exact_combine_external_body_identity(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    scope_record_index: u32,
    record_index: u32,
) -> Option<DesignCombineExternalBodyIdentity> {
    if bytes.get(
        start + combine_external::ZERO_RUN_14..start + combine_external::NESTED_REFERENCE_MARKER,
    )? != [0; 14]
    {
        return None;
    }
    let mut cursor = start.checked_add(combine_external::NESTED_REFERENCE_MARKER)?;
    let nested = take_reference(bytes, &mut cursor)?;
    if nested.target != Some(u64::from(record_index.checked_add(3)?))
        || nested.segment.is_some()
        || nested.link_name.is_some()
        || View::u32_le_at(bytes, cursor)? != 1
    {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let selector_asset_at = cursor;
    let (selector_asset_id, after_selector_asset_id) =
        lp_utf16_bounded(bytes, selector_asset_at, 1..=256)?;
    let selector_context_at = after_selector_asset_id;
    let (selector_context_id, after_selector_context_id) =
        lp_utf16_bounded(bytes, selector_context_at, 1..=256)?;
    if !is_guid_relaxed(&selector_asset_id)
        || !is_guid_relaxed(&selector_context_id)
        || View::u32_le_at(bytes, after_selector_context_id)? != 2
        || View::u32_le_at(bytes, after_selector_context_id.checked_add(4)?)? != 0
        || View::u32_le_at(bytes, after_selector_context_id.checked_add(8)?)? != 1
    {
        return None;
    }
    cursor = after_selector_context_id.checked_add(12)?;
    let occurrence_reference_at = cursor.checked_add(1)?;
    let occurrence = take_reference(bytes, &mut cursor)?;
    let occurrence_reference = occurrence.target?;
    if occurrence_reference == 0
        || occurrence.segment.is_some()
        || occurrence.link_name.is_some()
        || View::u32_le_at(bytes, cursor)? != 1
    {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let external = take_external_reference_identity(bytes, &mut cursor)?;
    if external.asset_id != selector_asset_id
        || View::u32_le_at(bytes, cursor)? != 9
        || View::u16_le_at(bytes, cursor.checked_add(4)?)? != 2
    {
        return None;
    }
    cursor = cursor.checked_add(6)?;
    let first_tail_value_at = cursor;
    let first_tail_value = View::u64_le_at(bytes, cursor)?;
    cursor = cursor.checked_add(8)?;
    if View::u32_le_at(bytes, cursor)? != 48 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let second_tail_value_at = cursor;
    let second_tail_value = View::u64_le_at(bytes, cursor)?;
    cursor = cursor.checked_add(8)?;
    let take_local = |cursor: &mut usize, expected| {
        let reference = take_reference(bytes, cursor)?;
        (reference.target == Some(u64::from(expected))
            && reference.segment.is_none()
            && reference.link_name.is_none())
        .then_some(())
    };
    take_local(&mut cursor, record_index.checked_add(2)?)?;
    if bytes.get(cursor..cursor.checked_add(2)?)? != [0; 2] {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    take_local(&mut cursor, record_index.checked_add(1)?)?;
    if bytes.get(cursor) != Some(&0) {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    take_local(&mut cursor, scope_record_index)?;
    if cursor != paired_at {
        return None;
    }
    Some(DesignCombineExternalBodyIdentity {
        selector_asset_id,
        selector_asset_id_offset: u64::try_from(selector_asset_at.checked_add(4)?).ok()?,
        selector_context_id,
        selector_context_id_offset: u64::try_from(selector_context_at.checked_add(4)?).ok()?,
        occurrence_reference,
        occurrence_reference_offset: u64::try_from(occurrence_reference_at).ok()?,
        external_body_reference: external.target,
        external_body_reference_offset: external.target_offset,
        external_segment: external.segment,
        external_segment_offset: external.segment_offset,
        external_asset_id: external.asset_id,
        external_asset_id_offset: external.asset_id_offset,
        external_link_name: external.link_name,
        external_link_name_offset: external.link_name_offset,
        external_version: external.version,
        tail_values: [first_tail_value, second_tail_value],
        tail_value_offsets: [
            u64::try_from(first_tail_value_at).ok()?,
            u64::try_from(second_tail_value_at).ok()?,
        ],
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
        && View::u32_le_at(frame, 21)? == 1
        && frame.get(25) == Some(&1)
        && frame.get(26..30)? == selection_reference
        && frame.get(30..36)? == [0; 6]
    {
        return Some(CombineOperandRole::Target);
    }
    if frame.get(11..20)? != [0; 9] || frame.get(20) != Some(&1) || View::u32_le_at(frame, 21)? != 1
    {
        return None;
    }
    let (property, after_property) = lp_ascii_filtered(frame, 25, 0..=2000, u8::is_ascii_graphic)?;
    let (property_type, after_property_type) =
        lp_ascii_filtered(frame, after_property, 0..=2000, u8::is_ascii_graphic)?;
    let count_at = after_property_type.checked_add(8)?;
    if property != "DcFeatureOperationIdFlag"
        || property_type != "IntrinsicMetaTypeuint64"
        || View::u32_le_at(frame, count_at)? != 1
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
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Draft)
        || scope.reference_members.len() < 6
    {
        return None;
    }
    let scope_stream = native_stream(&scope.id);
    let mut lanes = scope
        .reference_members
        .values()
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

pub(crate) fn parameter_scope_tail_length_is_valid(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> bool {
    let kind = kind.as_ref();
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
    kind: impl AsRef<str>,
    tail_length: usize,
) -> Option<usize> {
    parameter_scope_previous_history_offset_for_form(kind.as_ref(), tail_length, false)
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
    let kind_scan_start = paired_at
        .saturating_sub(590 + 4 + 2 * 256)
        .max(start.checked_add(11)?);
    let kind_scan_end = paired_at.checked_sub(72)?;
    for at in kind_scan_start..kind_scan_end {
        let Some((kind, kind_end)) = lp_utf16_bounded(bytes, at, 1..=256) else {
            continue;
        };
        if !kind.chars().all(|character| !character.is_control()) {
            continue;
        }
        let Some(tail_length) = paired_at.checked_sub(kind_end) else {
            continue;
        };
        let fixed_tail = matches!(tail_length, 72 | 76 | 77 | 78 | 82 | 87 | 88 | 104 | 110);
        if fixed_tail && parameter_scope_tail_length_is_valid(&kind, tail_length) {
            candidates.push((at, kind_end, tail_length, kind.clone(), false));
        }
        let named_tail = (78..=590).contains(&tail_length)
            && tail_length.is_multiple_of(2)
            && (parameter_scope_tail_length_is_valid(&kind, tail_length) || tail_length == 78)
            && named_parameter_scope_tail_is_valid(bytes, kind_end, paired_at, tail_length)
                .is_some_and(|valid| valid);
        if named_tail {
            candidates.push((at, kind_end, tail_length, kind, true));
        }
    }
    if candidates.iter().filter(|candidate| candidate.4).count() == 1 {
        candidates.retain(|candidate| candidate.4);
    }
    let [(kind_at, kind_end, tail_length, kind, named_tail)] = candidates.as_slice() else {
        return None;
    };
    let kind_text = kind.clone();
    let kind = crate::records::DesignFeatureKind::try_from(kind_text.clone()).ok()?;
    let kind_end = *kind_end;
    let reference_table_end = kind_at.checked_sub(4)?;
    let feature_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, kind_end)?)?;
    let history_state_id_offset = reference_table_end;
    let history_state_id = match View::u32_le_at(bytes, history_state_id_offset)? {
        u32::MAX => None,
        state_id => Some(i64::from(state_id)),
    };
    let previous_history_state_id_offset = match parameter_scope_previous_history_offset_for_form(
        &kind_text,
        *tail_length,
        *named_tail,
    ) {
        Some(offset) => Some(kind_end.checked_add(offset)?),
        None => None,
    };
    let previous_history_state_id =
        previous_history_state_id_offset.and_then(|offset| match View::u32_le_at(bytes, offset)? {
            u32::MAX => None,
            state_id => Some(i64::from(state_id)),
        });
    let mut reference_tables = Vec::new();
    for count_at in start + 11..reference_table_end {
        let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
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
            members.push(View::u32_le_at(bytes, marker + 1)?);
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
    let surface_stitch_operation = if kind == crate::records::DesignFeatureKind::SurfaceStitch {
        exact_surface_stitch_operation(bytes, records, header.record_index, reference_members)
    } else {
        None
    };
    let surface_patch_boundaries = if kind == crate::records::DesignFeatureKind::SurfacePatch {
        super::patch::surface_patch_boundaries(bytes, records, reference_members)
    } else {
        Vec::new()
    };
    let base_flange_operation = if kind == crate::records::DesignFeatureKind::BaseFlange {
        exact_base_flange_operation(bytes, start, paired_at, reference_members)
    } else {
        None
    };
    let edge_flange_operation = if kind == crate::records::DesignFeatureKind::EdgeFlange {
        exact_edge_flange_operation(
            bytes,
            start,
            paired_at,
            &header.class_tag,
            &paired_class_tag,
            reference_members,
        )
    } else {
        None
    };
    let ruled_surface_operation = if kind == crate::records::DesignFeatureKind::SurfaceRuled {
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
    let family = design_feature_family(&kind);
    // A `Sketch` scope carries either the single entity-suffix reference form
    // or, when the stream's sketch entity headers use the `EntityGenesis`
    // form, the generic ordered reference table. Both parse here; the entity
    // binding in `decode_parameter_scopes` requires a unique suffix match.
    let extrude_prologue = if family == Some(DesignFeatureFamily::Extrude) {
        // The generic scope envelope is independently self-delimiting. An
        // unrecognized Extrude prologue therefore withholds only the typed
        // fields, not the scope and its ordered reference table.
        exact_extrude_prologue(
            bytes,
            start,
            paired_at,
            &header.class_tag,
            &paired_class_tag,
            *reference_count_at,
            reference_members,
        )
    } else {
        None
    };
    let coil_discriminators = if family == Some(DesignFeatureFamily::Coil) {
        exact_coil_discriminators(bytes, start, paired_at, &kind, reference_members)
    } else {
        None
    };
    let coil_transform = if family == Some(DesignFeatureFamily::Coil) {
        exact_long_coil_transform(bytes, start, paired_at, &kind, reference_members)
    } else {
        None
    };
    let coil = if family == Some(DesignFeatureFamily::Coil) {
        Some(crate::records::DesignCoilScope {
            coil_operation: coil_discriminators.as_ref().map(|fields| crate::records::RecordedValue { value: fields.operation, offset: Some(fields.operation_offset) }),
            coil_extent: coil_discriminators.as_ref().and_then(|fields| fields.extent),
            coil_section: coil_discriminators.as_ref().map(|fields| crate::records::RecordedValue { value: fields.section, offset: fields.section_offset }),
            coil_section_placement: coil_discriminators.as_ref().map(|fields| crate::records::RecordedValue { value: fields.section_placement, offset: fields.section_placement_offset }),
            coil_clockwise: coil_discriminators.as_ref().map(|fields| crate::records::RecordedValue { value: fields.clockwise, offset: fields.clockwise_offset }),
            coil_placement: None,
            coil_transform,
        })
    } else {
        None
    };
    let mut scope = DesignParameterScope {
        id: String::new(),
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        record_index: header.record_index,
        frame_length: u64::try_from(paired_at.checked_sub(start)?).ok()?,
        kind_offset: u64::try_from(kind_at.checked_add(4)?).ok()?,
        feature_ordinal,
        feature_ordinal_offset: u64::try_from(kind_end).ok()?,
        history_state_id,

        previous_history_state_id,
        previous_history_state_id_offset: previous_history_state_id_offset
            .and_then(|offset| u64::try_from(offset).ok())
            .filter(|&offset| offset != 0),
        reference_count_offset: u64::try_from(*reference_count_at).ok()?,
        reference_members: crate::records::ReferenceRun::Located(reference_members.iter().copied()
            .zip(reference_member_offsets.iter().copied())
            .map(|(value, offset)| crate::records::Located { value, offset }).collect()),
        payload: kind.into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag,
        paired_byte_offset: paired_at as u64,
    };
    if let Some(prologue) = extrude_prologue {
        {
            let construction = Some(crate::records::DesignExtrudeScope {
                extrude_prologue: Some(prologue),
                ..crate::records::DesignExtrudeScope::default()
            });
            if let crate::records::DesignScopePayload::Extrude(slot)
            | crate::records::DesignScopePayload::Extrusion(slot)
            | crate::records::DesignScopePayload::Extrusao(slot) = &mut scope.payload
            {
                *slot = construction;
            }
        }
    }
    if let Some(coil) = coil {
        {
            let construction = Some(coil);
            if let crate::records::DesignScopePayload::SpirePrimitive(slot)
            | crate::records::DesignScopePayload::CoilPrimitive(slot) = &mut scope.payload
            {
                *slot = construction;
            }
        }
    }
    if let Some(operation) = surface_stitch_operation {
        {
            let construction = Some(operation);
            if let crate::records::DesignScopePayload::SurfaceStitch(slot) = &mut scope.payload {
                *slot = construction;
            }
        }
    }
    if let Some(operation) = ruled_surface_operation {
        {
            let construction = Some(operation);
            if let crate::records::DesignScopePayload::SurfaceRuled(slot) = &mut scope.payload {
                *slot = construction;
            }
        }
    }
    if !surface_patch_boundaries.is_empty() {
        {
            let construction = surface_patch_boundaries;
            if let crate::records::DesignScopePayload::SurfacePatch(slot) = &mut scope.payload {
                *slot = construction;
            }
        }
    }
    if let Some(operation) = base_flange_operation {
        {
            let construction = Some(crate::records::DesignBaseFlangeScope {
                base_flange_operation: Some(operation),
                ..crate::records::DesignBaseFlangeScope::default()
            });
            if let crate::records::DesignScopePayload::BaseFlange(slot) = &mut scope.payload {
                *slot = construction;
            }
        }
    }
    if let Some(operation) = edge_flange_operation {
        {
            let construction = Some(operation);
            if let crate::records::DesignScopePayload::EdgeFlange(slot) = &mut scope.payload {
                *slot = construction;
            }
        }
    }
    Some(scope)
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
    let first_lane_value = View::u64_le_at(bytes, marker + 2)?;
    let second_lane_value = View::u64_le_at(bytes, marker + 34)?;
    let third_lane_value = View::u64_le_at(bytes, marker + 48)?;
    Some(
        bytes.get(kind_end + 4..kind_end + 8)? == [0; 4]
            && bytes.get(marker) == Some(&1)
            && bytes.get(marker + 1).is_some_and(|field_id| *field_id != 0)
            && matches!(first_lane_value, 0 | 1)
            && second_lane_value == first_lane_value
            && third_lane_value == first_lane_value
            && bytes.get(marker + 10..marker + 12)? == [0; 2]
            && View::u32_le_at(bytes, marker + 12)? > 0
            && View::u32_le_at(bytes, marker + 16)? == 0xfc
            && View::f64_le_at(bytes, marker + 20)?.is_finite()
            && View::u32_le_at(bytes, marker + 28)? == 0xfc
            && bytes.get(marker + 32) == Some(&1)
            && bytes
                .get(marker + 33)
                .is_some_and(|field_id| *field_id != 0)
            && bytes.get(marker + 42..marker + 46)? == [0, 1, 0, 0]
            && bytes.get(marker + 46) == Some(&1)
            && bytes
                .get(marker + 47)
                .is_some_and(|field_id| *field_id != 0)
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
    extent: Option<crate::records::RecordedValue<DesignCoilExtent>>,
    section: DesignCoilSection,
    section_offset: Option<u64>,
    section_placement: DesignCoilSectionPlacement,
    section_placement_offset: Option<u64>,
    clockwise: bool,
    clockwise_offset: Option<u64>,
}

/// Decode the two ordered placement carriers of a compact Coil form.
///
/// The first carrier is a nested support-selection frame. It may carry either
/// a persistent support identity or a face recipe. The second is a rigid frame
/// whose direct identity form omits the matrix and therefore has a 213-byte
/// span. The 442-byte scope form also has an owner-referenced identity carrier:
/// it appends a marked reference to the owning scope and has a 233-byte span.
/// The explicit form has the same fixed envelope plus 128 matrix bytes and a
/// 341-byte span. The legacy 427-byte scope form has a class-395 identity
/// carrier with a 186-byte span. The modern 427-byte scope form has a
/// class-450 matrix carrier with a 315-byte span. A malformed or ambiguous
/// carrier leaves the complete placement native.
fn exact_coil_placement(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    recipes: &[ConstructionRecipe],
) -> Option<DesignCoilPlacement> {
    if scope.kind() != crate::records::DesignFeatureKind::CoilPrimitive {
        return None;
    }
    match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length,
        scope.reference_members.len(),
    ) {
        ("393", "258", 427, 8) => {}
        ("353", "259", 427, 8) => {}
        (_, _, 411, 7) if matches!(scope.coil_extent(), Some(DesignCoilExtent::Spiral)) => {}
        (_, _, 432 | 442, 8) => {}
        _ => return None,
    }
    let selection_record_index = *scope.reference_members.values().next()?;
    let transform_record_index = *scope.reference_members.values().nth(1)?;
    let selection_frames = records.frames(selection_record_index).collect::<Vec<_>>();
    let [(selection_start, _)] = selection_frames.as_slice() else {
        return None;
    };
    let selection_start = *selection_start;
    let (selection_class_tag, selection_after_tag) =
        lp_ascii_filtered(bytes, selection_start, 3..=3, u8::is_ascii_digit)?;
    if selection_after_tag != selection_start.checked_add(7)?
        || View::u32_le_at(bytes, selection_after_tag)? != selection_record_index
    {
        return None;
    }
    let transform_frames = records.frames(transform_record_index).collect::<Vec<_>>();
    let [(transform_start, transform_paired)] = transform_frames.as_slice() else {
        return None;
    };
    let transform_start = *transform_start;
    let transform_paired = *transform_paired;
    let (transform_class_tag, transform_after_tag) =
        lp_ascii_filtered(bytes, transform_start, 3..=3, u8::is_ascii_digit)?;
    if transform_after_tag != transform_start.checked_add(7)?
        || View::u32_le_at(bytes, transform_after_tag)? != transform_record_index
    {
        return None;
    }
    let transform_paired_class_tag =
        exact_indexed_header_at(bytes, transform_paired, transform_record_index)?;
    let frame_length = transform_paired.checked_sub(transform_start)?;
    let explicit_transform = match frame_length {
        coil_legacy_identity::LEN
            if scope.class_tag == "393"
                && scope.paired_class_tag == "258"
                && transform_class_tag == "395"
                && transform_paired_class_tag == "258"
                && exact_coil_legacy_identity_frame(
                    bytes,
                    transform_start,
                    transform_paired,
                    selection_record_index,
                    transform_record_index,
                    scope.record_index,
                ) =>
        {
            None
        }
        coil_modern_matrix::LEN
            if transform_class_tag == "450"
                && transform_paired_class_tag == "259"
                && exact_coil_modern_placement_matrix_frame(
                    bytes,
                    transform_start,
                    transform_paired,
                    selection_record_index,
                    transform_record_index,
                    scope.record_index,
                ) =>
        {
            let values = f64s_at(
                bytes,
                transform_start.checked_add(coil_modern_matrix::MATRIX)?,
                16,
            )?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            Some(crate::records::Located {
                value: transform,
                offset: u64::try_from(transform_start.checked_add(coil_modern_matrix::MATRIX)?).ok()?,
            })
        }
        coil_identity::LEN
            if bytes.get(transform_start + coil_identity::PLACEMENT_MARKER) == Some(&1)
                && bytes.get(
                    transform_start + coil_identity::IDENTITY_ZERO_RUN
                        ..transform_start + coil_identity::IDENTITY_MARKER,
                ) == Some(&[0; 9][..])
                && bytes.get(transform_start + coil_identity::IDENTITY_MARKER) == Some(&1) =>
        {
            None
        }
        coil_owner_identity::LEN
            if bytes.get(transform_start + coil_identity::PLACEMENT_MARKER) == Some(&1)
                && bytes.get(
                    transform_start + coil_identity::IDENTITY_ZERO_RUN
                        ..transform_start + coil_identity::IDENTITY_MARKER,
                ) == Some(&[0; 9][..])
                && bytes.get(transform_start + coil_identity::IDENTITY_MARKER) == Some(&1)
                && bytes.get(
                    transform_start + coil_identity::LEN
                        ..transform_start + coil_owner_identity::OWNER_REFERENCE_MARKER,
                ) == Some(&[0; 9][..])
                && bytes.get(transform_start + coil_owner_identity::OWNER_REFERENCE_MARKER)
                    == Some(&1)
                && View::u32_le_at(
                    bytes,
                    transform_start + coil_owner_identity::OWNER_SCOPE_RECORD_INDEX,
                ) == Some(scope.record_index)
                && bytes.get(
                    transform_start + coil_owner_identity::OWNER_REFERENCE_TAIL
                        ..transform_start + coil_owner_identity::LEN,
                ) == Some(&[0; 6][..]) =>
        {
            None
        }
        coil_matrix::LEN
            if bytes.get(transform_start + coil_matrix::PLACEMENT_MARKER) == Some(&1)
                && bytes.get(
                    transform_start + coil_matrix::EXPLICIT_ZERO_RUN
                        ..transform_start + coil_matrix::EXPLICIT_FORM_MARKER,
                ) == Some(&[0; 9][..])
                && bytes.get(transform_start + coil_matrix::EXPLICIT_FORM_MARKER) == Some(&0) =>
        {
            let values = f64s_at(bytes, transform_start.checked_add(coil_matrix::MATRIX)?, 16)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            Some(crate::records::Located {
                value: transform,
                offset: u64::try_from(transform_start.checked_add(coil_matrix::MATRIX)?).ok()?,
            })
        }
        _ => return None,
    };
    if explicit_transform.as_ref().is_some_and(|matrix| !valid_right_handed_coil_transform(&matrix.value)) {
        return None;
    }
    let selection = parse_entity_selection_frame(
        bytes,
        selection_record_index,
        u64::try_from(selection_start).ok()?,
        &selection_class_tag,
    )
    .map(|selection| DesignCoilSelection::Persistent {
        asset_id: selection.asset_id,
        context_id: selection.context_id,
        identity_record_index: selection.identity_record_index,
        primary_identity: selection.primary_identity,
        secondary: selection.secondary.map(|identity| crate::records::DesignSecondaryIdentity {
            identity: identity.identity.value,
            curve_identity: identity.curve_identity.map(|identity| identity.value),
        }),
    })
    .or_else(|| {
        exact_coil_face_selection(
            bytes,
            scope,
            selection_record_index,
            selection_start,
            &selection_class_tag,
            transform_start,
            recipes,
        )
    })?;
    Some(DesignCoilPlacement {
        selection_record_index,
        selection_record_byte_offset: u64::try_from(selection_start).ok()?,
        selection_class_tag,
        selection,
        transform_record_index,
        transform_record_byte_offset: u64::try_from(transform_start).ok()?,
        transform_class_tag,
        explicit_transform,
    })
}

fn exact_coil_modern_placement_matrix_frame(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    selection_record_index: u32,
    transform_record_index: u32,
    scope_record_index: u32,
) -> bool {
    paired_at.checked_sub(start) == Some(coil_modern_matrix::LEN)
        && bytes.get(start + 11..start + coil_modern_matrix::MATRIX) == Some(&[0; 39][..])
        && bytes.get(
            start + coil_modern_matrix::MATRIX + 16 * 8..start + coil_modern_matrix::CONSTANT_512,
        ) == Some(&[0; 26][..])
        && View::u32_le_at(bytes, start + coil_modern_matrix::CONSTANT_512) == Some(512)
        && bytes.get(
            start + coil_modern_matrix::CONSTANT_512 + 4..start + coil_modern_matrix::CONSTANT_256,
        ) == Some(&[0; 4][..])
        && View::u32_le_at(bytes, start + coil_modern_matrix::CONSTANT_256) == Some(256)
        && bytes.get(
            start + coil_modern_matrix::CONSTANT_256 + 4
                ..start + coil_modern_matrix::SELECTION_REFERENCE,
        ) == Some(&[0; 1][..])
        && marked_record_reference(bytes, start + coil_modern_matrix::SELECTION_REFERENCE)
            == Some(selection_record_index)
        && bytes.get(
            start + coil_modern_matrix::SELECTION_REFERENCE + 11
                ..start + coil_modern_matrix::SELECTION_FLAG,
        ) == Some(&[0; 2][..])
        && View::u32_le_at(bytes, start + coil_modern_matrix::SELECTION_FLAG) == Some(1)
        && marked_record_reference(bytes, start + coil_modern_matrix::AUXILIARY_REFERENCE)
            == transform_record_index.checked_add(25)
        && bytes.get(
            start + coil_modern_matrix::AUXILIARY_REFERENCE + 11
                ..start + coil_modern_matrix::CONSTANT_1024,
        ) == Some(&[0; 3][..])
        && View::u64_le_at(bytes, start + coil_modern_matrix::CONSTANT_1024) == Some(1024)
        && View::u64_le_at(bytes, start + coil_modern_matrix::IDENTITY_LANE_PREFIX)
            == Some(0x7000_0000_0000_0000)
        && bytes.get(
            start + coil_modern_matrix::IDENTITY_LANE_PREFIX + 8
                ..start + coil_modern_matrix::IDENTITY_LANE,
        ) == Some(&[0; 4][..])
        && View::u64_le_at(bytes, start + coil_modern_matrix::IDENTITY_LANE)
            .is_some_and(|value| value >> 56 == 0x70)
        && bytes.get(
            start + coil_modern_matrix::IDENTITY_LANE + 8
                ..start + coil_modern_matrix::SUCCESSOR_REFERENCE,
        ) == Some(&[0; 3][..])
        && marked_record_reference(bytes, start + coil_modern_matrix::SUCCESSOR_REFERENCE)
            == transform_record_index.checked_add(2)
        && bytes.get(
            start + coil_modern_matrix::SUCCESSOR_REFERENCE + 11
                ..start + coil_modern_matrix::PREDECESSOR_REFERENCE,
        ) == Some(&[0; 2][..])
        && marked_record_reference(bytes, start + coil_modern_matrix::PREDECESSOR_REFERENCE)
            == transform_record_index.checked_add(1)
        && bytes.get(
            start + coil_modern_matrix::PREDECESSOR_REFERENCE + 11
                ..start + coil_modern_matrix::OWNER_REFERENCE,
        ) == Some(&[0; 1][..])
        && marked_record_reference(bytes, start + coil_modern_matrix::OWNER_REFERENCE)
            == Some(scope_record_index)
}

fn exact_coil_legacy_identity_frame(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    selection_record_index: u32,
    transform_record_index: u32,
    scope_record_index: u32,
) -> bool {
    let Some(auxiliary_record_index) = marked_record_reference(
        bytes,
        start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER,
    ) else {
        return false;
    };
    paired_at.checked_sub(start) == Some(coil_legacy_identity::LEN)
        && bytes.get(start + 11..start + coil_legacy_identity::LEADING_REFERENCE_MARKER)
            == Some(&[0; 37][..])
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::LEADING_REFERENCE_MARKER,
        ) == Some(0)
        && bytes.get(
            start + coil_legacy_identity::LEADING_REFERENCE_MARKER + 11
                ..start + coil_legacy_identity::PROLOGUE_VALUE,
        ) == Some(&[0; 17][..])
        && View::u32_le_at(bytes, start + coil_legacy_identity::PROLOGUE_VALUE) == Some(2)
        && bytes.get(
            start + coil_legacy_identity::PROLOGUE_VALUE + 4
                ..start + coil_legacy_identity::PROLOGUE_FLAG,
        ) == Some(&[0; 4][..])
        && View::u32_le_at(bytes, start + coil_legacy_identity::PROLOGUE_FLAG) == Some(1)
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::SELECTION_REFERENCE_MARKER,
        ) == Some(selection_record_index)
        && bytes.get(
            start + coil_legacy_identity::SELECTION_RECORD_INDEX + 4
                ..start + coil_legacy_identity::SELECTION_REFERENCE_MARKER + 11,
        ) == Some(&[0; 6][..])
        && bytes.get(
            start + coil_legacy_identity::SELECTION_REFERENCE_MARKER + 11
                ..start + coil_legacy_identity::SELECTION_FLAG,
        ) == Some(&[0; 2][..])
        && View::u32_le_at(bytes, start + coil_legacy_identity::SELECTION_FLAG) == Some(1)
        && auxiliary_record_index != 0
        && auxiliary_record_index != selection_record_index
        && auxiliary_record_index != transform_record_index
        && auxiliary_record_index != scope_record_index
        && bytes.get(
            start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER + 5
                ..start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER + 11,
        ) == Some(&[0; 6][..])
        && bytes.get(
            start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER + 11
                ..start + coil_legacy_identity::TAIL_VALUE,
        ) == Some(&[0; 4][..])
        && View::u32_le_at(bytes, start + coil_legacy_identity::TAIL_VALUE) == Some(4)
        && bytes.get(
            start + coil_legacy_identity::TAIL_VALUE + 4
                ..start + coil_legacy_identity::INTERMEDIATE_SELECTOR,
        ) == Some(&[0; 10][..])
        && View::u32_le_at(bytes, start + coil_legacy_identity::INTERMEDIATE_SELECTOR) == Some(109)
        && View::f64_le_at(bytes, start + coil_legacy_identity::CARRIER_SCALAR)
            .is_some_and(|value| value.is_finite() && value > 0.0)
        && View::u32_le_at(bytes, start + coil_legacy_identity::TAIL_SELECTOR) == Some(109)
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::SUCCESSOR_REFERENCE_MARKER,
        ) == transform_record_index.checked_add(2)
        && bytes.get(
            start + coil_legacy_identity::SUCCESSOR_REFERENCE_MARKER + 11
                ..start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER,
        ) == Some(&[0; 2][..])
        && marked_record_reference(
            bytes,
            start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER,
        ) == transform_record_index.checked_add(1)
        && bytes.get(
            start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER + 5
                ..start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER + 11,
        ) == Some(&[0; 6][..])
        && bytes.get(start + coil_legacy_identity::OWNER_REFERENCE_MARKER - 1) == Some(&0)
        && marked_record_reference(bytes, start + coil_legacy_identity::OWNER_REFERENCE_MARKER)
            == Some(scope_record_index)
}

fn exact_coil_face_selection(
    bytes: &[u8],
    scope: &DesignParameterScope,
    selection_record_index: u32,
    selection_start: usize,
    selection_class_tag: &str,
    transform_start: usize,
    recipes: &[ConstructionRecipe],
) -> Option<DesignCoilSelection> {
    let prefix = parse_entity_selection_prefix(bytes, selection_start, selection_record_index)?;
    let header = DesignRecordHeader {
        id: scope.id.clone(),
        byte_offset: u64::try_from(selection_start).ok()?,
        class_tag: selection_class_tag.to_owned(),
        record_index: selection_record_index,
    };
    let face = parse_face_operand(
        bytes,
        &IndexedRecordOffsets::build(bytes),
        scope,
        0,
        None,
        Some(u64::try_from(transform_start).ok()?),
        &header,
        recipes,
    )?;
    if face.next_byte_offset != u64::try_from(transform_start).ok()? {
        return None;
    }
    let recipe = recipes.iter().find(|recipe| recipe.id == face.recipe_id)?;
    Some(DesignCoilSelection::FaceRecipe {
        asset_id: prefix.asset_id,
        context_id: prefix.context_id,
        recipe_record_index: face.recipe_record_index,
        recipe_record_byte_offset: face.recipe_record_byte_offset,
        recipe_id: recipe.id.clone(),
        recipe_kind: crate::records::DesignFaceRecipeKind::try_from(recipe.kind).ok()?,
        design: recipe.design.as_ref().map(|design| crate::records::ConstructionRecipeDesign {
            id: design.id.value.clone(),
            selector: design.selector,
        }),
    })
}

fn valid_right_handed_coil_transform(transform: &[[f64; 4]; 4]) -> bool {
    if !valid_sketch_transform(transform) {
        return false;
    }
    let radial = [transform[0][0], transform[1][0], transform[2][0]];
    let tangent = [transform[0][1], transform[1][1], transform[2][1]];
    let axis = [transform[0][2], transform[1][2], transform[2][2]];
    let cross = [
        radial[1] * tangent[2] - radial[2] * tangent[1],
        radial[2] * tangent[0] - radial[0] * tangent[2],
        radial[0] * tangent[1] - radial[1] * tangent[0],
    ];
    cross
        .into_iter()
        .zip(axis)
        .map(|(left, right)| left * right)
        .sum::<f64>()
        > 1.0 - EPS_SCOPES_VALID_RIGHT_HANDED_COIL_TRANSFORM_E10
}

fn exact_coil_discriminators(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &crate::records::DesignFeatureKind,
    reference_members: &[u32],
) -> Option<CoilDiscriminators> {
    if let Some(fields) =
        exact_long_coil_discriminators(bytes, start, paired_at, kind, reference_members)
    {
        return Some(fields);
    }
    let operation_offset = start.checked_add(coil_compact::OPERATION)?;
    let operation = match (kind, View::u32_le_at(bytes, operation_offset)?) {
        (&crate::records::DesignFeatureKind::SpirePrimitive, 1) => DesignExtrudeOperation::Join,
        (&crate::records::DesignFeatureKind::SpirePrimitive, 2) => DesignExtrudeOperation::Cut,
        (&crate::records::DesignFeatureKind::SpirePrimitive, 3) => {
            DesignExtrudeOperation::Intersect
        }
        (&crate::records::DesignFeatureKind::SpirePrimitive, 4)
        | (&crate::records::DesignFeatureKind::CoilPrimitive, 1) => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    let clockwise_offset = start.checked_add(coil_compact::CLOCKWISE)?;
    let clockwise = match bytes.get(clockwise_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let structural_constant = match kind {
        crate::records::DesignFeatureKind::SpirePrimitive => 2,
        crate::records::DesignFeatureKind::CoilPrimitive => 4,
        _ => return None,
    };
    if View::u32_le_at(bytes, start.checked_add(coil_compact::STRUCTURAL_CONSTANT)?)?
        != structural_constant
    {
        return None;
    }
    let extent_offset = start.checked_add(coil_compact::EXTENT)?;
    let extent = match View::u32_le_at(bytes, extent_offset)? {
        1 => DesignCoilExtent::RevolutionsHeight,
        2 => DesignCoilExtent::RevolutionsPitch,
        3 => DesignCoilExtent::HeightPitch,
        4 => DesignCoilExtent::Spiral,
        _ => return None,
    };
    let section_offset = start.checked_add(coil_compact::SECTION_PLACEMENT)?;
    let section_placement_offset = start.checked_add(coil_compact::SECTION_SHAPE)?;
    let (section, section_placement) = match kind {
        crate::records::DesignFeatureKind::SpirePrimitive => (
            match View::u32_le_at(bytes, section_offset)? {
                0 => DesignCoilSection::Circular,
                1 => DesignCoilSection::Square,
                2 => DesignCoilSection::ExternalTriangle,
                3 => DesignCoilSection::InternalTriangle,
                _ => return None,
            },
            match View::u32_le_at(bytes, section_placement_offset)? {
                4 => DesignCoilSectionPlacement::Inside,
                _ => return None,
            },
        ),
        // The compact Coil dialect stores the two discriminators in the
        // opposite lanes from SpirePrimitive: position at offset 92 and
        // section shape at offset 107.
        crate::records::DesignFeatureKind::CoilPrimitive => (
            match View::u32_le_at(bytes, section_placement_offset)? {
                1 => DesignCoilSection::Circular,
                2 => DesignCoilSection::Square,
                3 => DesignCoilSection::ExternalTriangle,
                4 => DesignCoilSection::InternalTriangle,
                _ => return None,
            },
            match View::u32_le_at(bytes, section_offset)? {
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
        extent: Some(crate::records::RecordedValue { value: extent, offset: Some(extent_offset as u64) }),
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
    kind: &crate::records::DesignFeatureKind,
    reference_members: &[u32],
) -> Option<CoilDiscriminators> {
    if *kind != crate::records::DesignFeatureKind::CoilPrimitive || reference_members.len() != 10 {
        return None;
    }
    let frame_length = paired_at.checked_sub(start)?;
    if !matches!(frame_length, 450 | 572 | 578)
        || bytes.get(
            start.checked_add(coil_long::ZERO_RUN_11)?..start.checked_add(coil_long::OPERATION)?,
        )? != [0; 11]
        || View::u32_le_at(bytes, start.checked_add(coil_long::STRUCTURAL_CONSTANT)?)? != 1
        || marked_record_reference(bytes, start.checked_add(coil_long::FIFTH_REFERENCE)?)?
            != *reference_members.get(4)?
        || marked_record_reference(bytes, start.checked_add(coil_long::NINTH_REFERENCE)?)?
            != *reference_members.get(8)?
    {
        return None;
    }
    let matrix_form = matches!(frame_length, 572 | 578) && exact_long_coil_matrix(bytes, start);
    let operation_value = View::u32_le_at(bytes, start.checked_add(coil_long::OPERATION)?)?;
    let operation = match (frame_length, operation_value) {
        (450, 1) => DesignExtrudeOperation::Join,
        (450, 2) => DesignExtrudeOperation::Cut,
        (450, 3) => DesignExtrudeOperation::Intersect,
        (572, 1) if matrix_form => DesignExtrudeOperation::Join,
        (572, 2) if matrix_form => DesignExtrudeOperation::Cut,
        (572, 3) if matrix_form => DesignExtrudeOperation::Intersect,
        (578, 2) if matrix_form => DesignExtrudeOperation::NewBody,
        _ => return None,
    };
    Some(CoilDiscriminators {
        operation,
        operation_offset: u64::try_from(start.checked_add(coil_long::OPERATION)?).ok()?,
        // The long form has no extent selector. Its exact owned parameter set
        // supplies the mode after the scope is parsed.
        extent: None,
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

fn exact_long_coil_transform(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    kind: &crate::records::DesignFeatureKind,
    reference_members: &[u32],
) -> Option<crate::records::DesignCoilTransform> {
    if *kind != crate::records::DesignFeatureKind::CoilPrimitive
        || reference_members.len() != 10
        || !matches!(paired_at.checked_sub(start)?, 572 | 578)
    {
        return None;
    }
    let transform = exact_long_coil_transform_values(bytes, start)?;
    Some(crate::records::DesignCoilTransform {
        transform,
        transform_offset: u64::try_from(start.checked_add(77)?).ok()?,
    })
}

fn exact_long_coil_transform_values(bytes: &[u8], start: usize) -> Option<[[f64; 4]; 4]> {
    let values = f64s_at(bytes, start.checked_add(77)?, 16)?;
    if !exact_long_coil_matrix(bytes, start) {
        return None;
    }
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    valid_right_handed_coil_transform(&transform).then_some(transform)
}

fn bind_coil_extent_from_parameters(
    scope: &mut DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::DesignParameterOwner],
) {
    if scope.kind() != crate::records::DesignFeatureKind::CoilPrimitive
        || scope.coil_extent().is_some()
    {
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
                .map(|parameter| (owner.local_ordinal, parameter.source_kind()))
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
    if let Some(extent) = extent {
        if let crate::records::DesignScopePayload::SpirePrimitive(slot)
        | crate::records::DesignScopePayload::CoilPrimitive(slot) = &mut scope.payload
        {
            slot.get_or_insert_with(Default::default).coil_extent = Some(crate::records::RecordedValue {
                value: extent,
                offset: None,
            });
        }
    }
}

pub(crate) fn marked_record_reference(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || bytes.get(at + 5..at + 11)? != [0; 6] {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}

pub(crate) fn parameter_scope_payload_length(scope: &DesignParameterScope) -> Option<u64> {
    let kind_bytes = u64::try_from(scope.kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    scope.frame_length.checked_sub(kind_bytes)
}

mod base_feature;

#[cfg(test)]
mod tests;
