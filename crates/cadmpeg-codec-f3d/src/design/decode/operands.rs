// SPDX-License-Identifier: Apache-2.0
//! Parse edge, face, and body operand frames and recipe structure.

use crate::bytes::{is_guid_relaxed, lp_ascii_filtered, lp_utf16_bounded, take_reference};
use crate::container::{role, ContainerScan};
use crate::design::decode::dimension_frames::{
    bind_recipe_reference_candidates, contiguous_i32_program, decode_recipe_references,
    recipe_record_prefix,
};
use crate::design::decode::scopes::extrude_sheet_metal::is_class_296_two_sided_to_faces_scope;
use crate::design::decode::scopes::payload_prologue;
use crate::design::decode::sketch::{
    indexed_record_index, next_indexed_record_offset, next_indexed_record_offset_with_index,
    IndexedRecordOffsets,
};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::layout::class_338_sketch_curve_identity as class_338_curve;
use crate::layout::coil_compact_face_selection_prefix as coil_face_sel;
use crate::layout::coil_compact_persistent_selection_prefix as coil_persist_sel;
use crate::layout::coil_modern_selection_prefix as coil_modern_sel;
use crate::layout::extrude_selection_member_fixed_frame as extrude_member;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::legacy_loft_body_carrier_class_322 as legacy_loft_322;
use crate::layout::legacy_loft_body_carrier_class_322_tail as legacy_loft_322_tail;
use crate::layout::legacy_loft_body_carrier_class_411 as legacy_loft_411;
use crate::layout::sketch_profile_region_member as region_member;
use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::layout::work_point_sketch_point_identity as sketch_point_identity;
use crate::records::{
    ConstructionRecipe, ConstructionRecipeKind, DesignBodyRecipeOperand, DesignBodyRecipeReference,
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandIdentity, DesignConstructionPersistentIdentity,
    DesignConstructionTrackingPath, DesignEdgeIdentityOperand, DesignEdgeOperand,
    DesignEdgeTreatmentVertexOperand, DesignEntityHeader, DesignEntitySelectionOperand,
    DesignExtrudeExtent, DesignExtrudeFaceRole, DesignExtrudeOperandRole, DesignExtrudePrologue,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignExtrudeStart,
    DesignFaceOperand, DesignFaceSourceGroup, DesignFaceSourceMember, DesignFilletRadiusGroup,
    DesignFilletRadiusLaw, DesignLoftLegacyBodyCarrier, DesignOperandOwner, DesignParameter,
    DesignParameterOwner, DesignParameterScope, DesignPathFeatureConstruction, DesignRecordHeader,
    DesignSketchProfileOperand, DesignSketchProfileRegion, DesignSketchProfileRegionMember,
    DesignSketchProfileRegionSelection, DesignSurfaceOffsetSupport, DesignTopologyRecipeEntry,
    DesignTopologyRecipeSide, DesignTopologyRecipeTriplet, DesignVertexRecipe,
    DesignWorkPlaneConstruction, DesignWorkPointInputCarrier, DesignWorkPointPlaneSelection,
    DesignWorkPointSketchPointSelection, LostEdgeReference, PersistentSubentityTag,
    SketchCurveIdentity, SketchPoint, SketchRelationOperand,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode edge-recipe operand frames named by edge-selecting feature scopes.
pub fn decode_edge_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeOperand>, CodecError> {
    let record_headers = headers;
    let headers = record_headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let terminal_group_members = groups
        .iter()
        .filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.scope_record_index,
                *group.members.last()?,
            ))
        })
        .collect::<HashSet<_>>();
    let mut stream_offsets: HashMap<&str, Vec<u64>> = HashMap::new();
    for header in record_headers {
        if let Some(stream) = native_stream(&header.id) {
            stream_offsets
                .entry(stream)
                .or_default()
                .push(header.byte_offset);
        }
    }
    for offsets in stream_offsets.values_mut() {
        offsets.sort_unstable();
    }
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind))
    {
        let mut member_indices = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == native_stream(&scope.id)
                    && group.scope_record_index == scope.record_index
            })
            .flat_map(|group| group.members.iter().copied())
            .collect::<HashSet<_>>();
        if let Some(operation) = scope.surface_extend_operation() {
            member_indices.extend(operation.edge_record_indices.iter().copied());
        }
        if let Some(operation) = scope.surface_offset_operation() {
            if let DesignSurfaceOffsetSupport::BoundaryCarrier {
                edge_record_indices,
                ..
            } = &operation.support
            {
                member_indices.extend(edge_record_indices.iter().copied());
            }
        }
        if let Some(construction) = scope.work_point_construction() {
            member_indices.extend(
                construction
                    .rule
                    .inputs()
                    .iter()
                    .map(|input| input.record_index),
            );
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            if !member_indices.contains(&record_index) {
                continue;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let terminal_group_member = terminal_group_members.contains(&(
                stream.to_owned(),
                scope.record_index,
                header.record_index,
            ));
            let terminal_group_limit = terminal_group_member.then(|| {
                stream_offsets
                    .get(stream)
                    .and_then(|offsets| {
                        let at = offsets.partition_point(|offset| *offset <= header.byte_offset);
                        offsets.get(at).copied()
                    })
                    .unwrap_or_else(|| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            });
            let Some(operand) = parse_edge_operand(
                bytes,
                records,
                scope,
                ordinal,
                header,
                recipes,
                terminal_group_limit,
            ) else {
                continue;
            };
            out.push(operand);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode vertex-recipe members retained inside edge-treatment groups.
pub fn decode_edge_treatment_vertex_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignEdgeTreatmentVertexOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| has_edge_recipe_operands(&scope.kind))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        for (scope_reference_ordinal, record_index) in
            scope.reference_members.iter().copied().enumerate()
        {
            let matches = groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == Some(stream)
                        && group.scope_record_index == scope.record_index
                })
                .filter_map(|group| {
                    let mut ordinals = group
                        .members
                        .iter()
                        .enumerate()
                        .filter(|(_, member)| **member == record_index);
                    let (ordinal, _) = ordinals.next()?;
                    ordinals.next().is_none().then_some((group, ordinal))
                })
                .collect::<Vec<_>>();
            let [(group, group_member_ordinal)] = matches.as_slice() else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Some(recipe) = parse_vertex_recipe(bytes, records, stream, header, recipes) else {
                continue;
            };
            let (Ok(scope_reference_ordinal), Ok(group_member_ordinal)) = (
                u32::try_from(scope_reference_ordinal),
                u32::try_from(*group_member_ordinal),
            ) else {
                continue;
            };
            out.push(DesignEdgeTreatmentVertexOperand {
                id: crate::ids::native_scoped_id(
                    stream,
                    "edge-treatment-vertex-operand",
                    header.byte_offset,
                ),
                scope_record_index: scope.record_index,
                scope_reference_ordinal,
                group_record_index: group.record_index,
                group_member_ordinal,
                recipe,
            });
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

/// Bind each `WorkPoint` input to its exact edge, vertex, or `WorkPlane` carrier.
pub fn bind_work_point_input_carriers(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    edge_operands: &[DesignEdgeOperand],
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| {
            Some((
                (native_stream(&header.id)?.to_owned(), header.record_index),
                header,
            ))
        })
        .collect::<HashMap<_, _>>();
    let work_planes = scopes
        .iter()
        .filter(|scope| scope.kind == "WorkPlane")
        .filter_map(|scope| {
            Some((
                (
                    native_stream(&scope.id)?.to_owned(),
                    scope.record_index.checked_sub(1)?,
                ),
                scope.record_index,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();

    for scope in scopes.iter_mut().filter(|scope| scope.kind == "WorkPoint") {
        let Some(stream) = native_stream(&scope.id).map(str::to_owned) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, &stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream.clone())
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        let scope_record_index = scope.record_index;
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        for input in construction.rule.inputs_mut() {
            let edge_matches = edge_operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == Some(stream.as_str())
                        && operand.scope_record_index == scope_record_index
                        && operand.record_index == input.record_index
                })
                .collect::<Vec<_>>();
            if let [operand] = edge_matches.as_slice() {
                input.carrier = Some(Box::new(DesignWorkPointInputCarrier::EdgeRecipe {
                    operand_id: operand.id.clone(),
                }));
                continue;
            }
            let Some(header) = headers.get(&(stream.clone(), input.record_index)) else {
                continue;
            };
            if let Some(recipe) = parse_vertex_recipe(bytes, records, &stream, header, recipes) {
                input.carrier = Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                    recipe,
                }));
                continue;
            }
            if let Some(selection) = parse_work_point_sketch_point_frame(
                bytes,
                input.record_index,
                header.byte_offset,
                &header.class_tag,
            ) {
                let point_matches = sketch_points
                    .iter()
                    .filter(|point| {
                        native_stream(&point.id) == Some(stream.as_str())
                            && point.owner_reference == Some(selection.sketch_record_index)
                            && point.persistent_id() == Some(selection.point_persistent_id)
                    })
                    .collect::<Vec<_>>();
                if let [point] = point_matches.as_slice() {
                    input.carrier = Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
                        selection: DesignWorkPointSketchPointSelection {
                            class_tag: selection.class_tag,
                            asset_id: selection.asset_id,
                            asset_id_offset: selection.asset_id_offset,
                            context_id: selection.context_id,
                            context_id_offset: selection.context_id_offset,
                            identity_record_index: selection.identity_record_index,
                            identity_record_offset: selection.identity_record_offset,
                            sketch_record_index: selection.sketch_record_index,
                            sketch_record_index_offset: selection.sketch_record_index_offset,
                            point_persistent_id: selection.point_persistent_id,
                            point_persistent_id_offset: selection.point_persistent_id_offset,
                            point_native_id: point.id.clone(),
                            next_record_index: selection.next_record_index,
                            next_byte_offset: selection.next_byte_offset,
                        },
                    }));
                }
                continue;
            }
            let Some(selection) = parse_entity_selection_frame(
                bytes,
                input.record_index,
                header.byte_offset,
                &header.class_tag,
            ) else {
                continue;
            };
            let Ok(primary_identity) = u32::try_from(selection.primary_identity) else {
                continue;
            };
            let Some(work_plane_scope_record_index) = work_planes
                .get(&(stream.clone(), primary_identity))
                .copied()
            else {
                continue;
            };
            if selection.secondary_identity.is_some()
                || selection.curve_secondary_identity.is_some()
            {
                continue;
            }
            input.carrier = Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
                selection: DesignWorkPointPlaneSelection {
                    class_tag: header.class_tag.clone(),
                    asset_id: selection.asset_id,
                    asset_id_offset: selection.asset_id_offset,
                    context_id: selection.context_id,
                    context_id_offset: selection.context_id_offset,
                    identity_record_index: selection.identity_record_index,
                    identity_record_offset: selection.identity_record_offset,
                    primary_identity: selection.primary_identity,
                    primary_identity_offset: selection.primary_identity_offset,
                    work_plane_scope_record_index,
                    next_record_index: selection.next_record_index,
                    next_byte_offset: selection.next_byte_offset,
                },
            }));
        }
    }
    Ok(())
}

/// Bind the exact three-vertex construction carried by a `WorkPlane` scope.
pub fn bind_work_plane_constructions(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| {
            Some((
                (native_stream(&header.id)?.to_owned(), header.record_index),
                header,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut record_offset_index: HashMap<String, IndexedRecordOffsets> = HashMap::new();

    for scope in scopes.iter_mut().filter(|scope| scope.kind == "WorkPlane") {
        if let Some(frame) = scope.work_plane_frame_mut() {
            frame.work_plane_construction = None;
        }
        let Some(stream) = native_stream(&scope.id).map(str::to_owned) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, &stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream.clone())
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        let [placement_record_index, first, second, third, extra_offset] =
            scope.reference_members.as_slice()
        else {
            continue;
        };
        if scope.work_plane_frame().is_none() || scope.work_plane_reference() != Some(*extra_offset)
        {
            continue;
        }
        let Some(owner) = owners.iter().find(|owner| {
            native_stream(&owner.id) == Some(stream.as_str())
                && owner.record_index == *extra_offset
                && owner.scope_record_index == scope.record_index
                && owner.evaluated_value.is_finite()
                && owner.evaluated_value == 0.0
        }) else {
            continue;
        };
        if !parameters.iter().any(|parameter| {
            native_stream(&parameter.id) == Some(stream.as_str())
                && parameter.record_index == owner.parameter_record_index
                && parameter.owner_record_index() == Some(owner.record_index)
                && parameter.source_kind == "ExtraOffset"
                && parameter.evaluated_value.is_finite()
                && parameter.evaluated_value == 0.0
        }) {
            continue;
        }
        let inputs = [first, second, third]
            .into_iter()
            .map(|record_index| {
                parse_vertex_recipe(
                    bytes,
                    records,
                    &stream,
                    headers.get(&(stream.clone(), *record_index))?,
                    recipes,
                )
            })
            .collect::<Option<Vec<_>>>();
        let Some(inputs) = inputs else {
            continue;
        };
        let Ok(inputs) = inputs.try_into() else {
            continue;
        };
        let placement_record_index = *placement_record_index;
        if let Some(frame) = scope.work_plane_frame_mut() {
            frame.work_plane_construction = Some(DesignWorkPlaneConstruction::ThreePoint {
                placement_record_index,
                inputs: Box::new(inputs),
            });
        }
    }
    Ok(())
}

/// Bind persistent subentity candidates carried by decoded vertex recipes.
pub fn bind_vertex_recipe_candidates(
    scopes: &mut [DesignParameterScope],
    tags: &[PersistentSubentityTag],
) {
    for scope in scopes {
        let scope_id = scope.id.clone();
        if let Some(DesignWorkPlaneConstruction::ThreePoint { inputs, .. }) =
            scope.work_plane_construction_mut()
        {
            for recipe in inputs.iter_mut() {
                for reference in &mut recipe.recipe_references {
                    bind_recipe_reference_candidates(reference, tags, Some(&scope_id));
                }
            }
        }
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        for recipe in construction
            .rule
            .inputs_mut()
            .iter_mut()
            .filter_map(|input| {
                let DesignWorkPointInputCarrier::VertexRecipe { recipe } =
                    input.carrier.as_deref_mut()?
                else {
                    return None;
                };
                Some(recipe)
            })
        {
            for reference in &mut recipe.recipe_references {
                bind_recipe_reference_candidates(reference, tags, Some(&scope_id));
            }
        }
    }
}

/// Bind active fallback candidates for edge-treatment corner recipes.
pub fn bind_edge_treatment_vertex_candidates(
    operands: &mut [DesignEdgeTreatmentVertexOperand],
    tags: &[PersistentSubentityTag],
) {
    for operand in operands {
        for reference in &mut operand.recipe.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
    }
}

/// Whether a feature family owns edge-recipe operands directly or through a
/// counted construction-operand group.
pub(crate) fn has_edge_recipe_operands(kind: impl AsRef<str>) -> bool {
    let kind = kind.as_ref();
    matches!(
        design_feature_family(kind),
        Some(
            DesignFeatureFamily::Fillet
                | DesignFeatureFamily::Chamfer
                | DesignFeatureFamily::Revolve
                | DesignFeatureFamily::Loft
                | DesignFeatureFamily::Sweep
                | DesignFeatureFamily::Pipe
                | DesignFeatureFamily::SurfacePatch
                | DesignFeatureFamily::SurfaceExtend
                | DesignFeatureFamily::SurfaceOffset
                | DesignFeatureFamily::SurfaceRuled
        )
    ) || matches!(kind, "EdgeFlange" | "Hem" | "WorkPoint")
}

/// Indexed-record distance from an edge-recipe primary record to its terminal
/// record for the owning consumer.
pub(crate) fn edge_recipe_terminal_delta(kind: impl AsRef<str>) -> u32 {
    let kind = kind.as_ref();
    match design_feature_family(kind) {
        Some(DesignFeatureFamily::Sweep) => 7,
        _ if kind == "WorkPoint" => 5,
        _ => 4,
    }
}

/// Decode persistent selection identities named by Fillet and Chamfer groups.
pub fn decode_edge_identity_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEdgeIdentityOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.record_index == group.scope_record_index
                && matches!(
                    design_feature_family(&scope.kind),
                    Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
                )
        }) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            let Some(parsed) = parse_edge_identity_member(bytes, start) else {
                continue;
            };
            let Ok(group_member_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            out.push(DesignEdgeIdentityOperand {
                id: ids::native_design_edge_identity_operand_id(&entry.name, header.byte_offset),
                scope_record_index: scope.record_index,
                group_record_index: group.record_index,
                group_member_ordinal,
                record_index,
                byte_offset: header.byte_offset,
                class_tag: header.class_tag.clone(),
                compact_layout: parsed.compact_layout,
                local_id: parsed.local_id,
                local_id_offset: parsed.local_id_offset,
                asset_id: parsed.asset_id,
                asset_id_offset: parsed.asset_id_offset,
                context_id: parsed.context_id,
                context_id_offset: parsed.context_id_offset,
                historical: None,
                treatment_radius_candidates: Vec::new(),
                transition_edge_candidates: Vec::new(),
                resolved_edge_slots: Vec::new(),
                resolved_edge_slot: None,
                resolution_identity_id: None,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode face-recipe operand frames named by grouped and direct feature references.
pub fn decode_face_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignFaceOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let scopes = scopes
        .iter()
        .filter_map(|scope| Some(((native_stream(&scope.id)?, scope.record_index), scope)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(scope) = scopes.get(&(stream, group.scope_record_index)) else {
            continue;
        };
        let is_extrude_operand = matches!(
            group.extrude_role,
            Some(DesignExtrudeOperandRole::Profile | DesignExtrudeOperandRole::Faces(_))
        );
        let is_offset_faces_operand = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::OffsetFaces)
            && group.role == 0x0000_0010_0000_0000;
        let is_shell_operand = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Shell)
            && group.role == 0x0000_0010_0000_0000;
        let is_loft_profile = design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Loft)
            && matches!(group.role, 0x0000_0041_0000_0000 | 0x0000_0043_0000_0000);
        let is_sweep_guide_surface = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Sweep)
            && group.role == 0x0000_0011_0000_0000;
        let is_revolve_axis = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Revolve)
            && group.role == 0x0000_0021_0000_0000;
        let is_edge_treatment_support = matches!(
            design_feature_family(&scope.kind),
            Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
        );
        let is_circular_pattern_seed = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::CircularPattern)
            && group.role == 0x0000_0008_0000_0000;
        let is_mirror_seed = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Mirror)
            && group.role == 0x0000_0008_0000_0000;
        let is_mirror_plane = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::Mirror)
            && group.role == 0x0000_0005_0000_0000;
        let is_split_face_operand = scope.kind == "SplitFace";
        let is_delete_face_operand =
            matches!(scope.kind.as_str(), "DeleteFace" | "SurfaceDeleteFace");
        let is_thread_face = scope.kind == "Thread" && group.role == 0x0000_0010_0000_0000;
        let is_hole_face = scope.kind == "Hole" && group.role == 0x0000_0004_0000_0000;
        let is_draft_operand =
            design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Draft);
        let is_replace_face_operand = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::ReplaceFace)
            && group.role == 0x0000_0010_0000_0000;
        let is_surface_offset_operand = design_feature_family(&scope.kind)
            == Some(DesignFeatureFamily::SurfaceOffset)
            && group.role == 0x0000_0041_0000_0000;
        if !is_extrude_operand
            && !is_offset_faces_operand
            && !is_shell_operand
            && !is_loft_profile
            && !is_sweep_guide_surface
            && !is_revolve_axis
            && !is_edge_treatment_support
            && !is_circular_pattern_seed
            && !is_mirror_seed
            && !is_mirror_plane
            && !is_split_face_operand
            && !is_delete_face_operand
            && !is_thread_face
            && !is_hole_face
            && !is_draft_operand
            && !is_replace_face_operand
            && !is_surface_offset_operand
        {
            continue;
        }
        if group.extrude_role == Some(DesignExtrudeOperandRole::Profile)
            && scope.extrude_profile().is_some()
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        for (group_member_index, record_index) in group.members.iter().enumerate() {
            if !seen.insert((stream, scope.record_index, *record_index)) {
                continue;
            }
            let Ok(group_member_ordinal) = u32::try_from(group_member_index) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            let next_byte_offset = group
                .members
                .get(group_member_index + 1)
                .and_then(|record_index| headers.get(&(stream, *record_index)))
                .copied()
                .map(|header| header.byte_offset)
                .or_else(|| {
                    if !is_offset_faces_operand && !is_shell_operand {
                        return None;
                    }
                    scope
                        .reference_members
                        .iter()
                        .position(|candidate| candidate == record_index)
                        .and_then(|ordinal| scope.reference_members.get(ordinal + 1))
                        .and_then(|record_index| headers.get(&(stream, *record_index)))
                        .map(|header| header.byte_offset)
                });
            if let Some(operand) = parse_face_operand(
                bytes,
                records,
                scope,
                group.scope_reference_ordinal,
                Some((group.record_index, group_member_ordinal)),
                next_byte_offset,
                header,
                recipes,
            ) {
                out.push(operand);
            }
        }
    }
    for scope in scopes.values().filter(|scope| {
        let is_legacy_as_built_421 = scope.kind == "As-built"
            && crate::design::assembly::legacy_as_built_421_generation(
                scope.frame_length,
                &scope.class_tag,
                &scope.paired_class_tag,
            )
            .is_some();
        matches!(
            design_feature_family(&scope.kind),
            Some(
                DesignFeatureFamily::OffsetFaces
                    | DesignFeatureFamily::Shell
                    | DesignFeatureFamily::Thicken
                    | DesignFeatureFamily::Split
                    | DesignFeatureFamily::ReplaceFace
            )
        ) || matches!(scope.kind.as_str(), "SplitFace" | "Hole")
            || is_legacy_as_built_421
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        let ordinals = if scope.kind == "As-built"
            && crate::design::assembly::legacy_as_built_421_generation(
                scope.frame_length,
                &scope.class_tag,
                &scope.paired_class_tag,
            )
            .is_some()
        {
            [1_usize, 3].into_iter().collect::<Vec<_>>()
        } else {
            (0..scope.reference_members.len()).collect::<Vec<_>>()
        };
        for ordinal in ordinals {
            let Some(record_index) = scope.reference_members.get(ordinal).copied() else {
                continue;
            };
            if !seen.insert((stream, scope.record_index, record_index)) {
                continue;
            }
            let (Ok(scope_reference_ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            let next_byte_offset = if scope.kind == "As-built"
                && crate::design::assembly::legacy_as_built_421_generation(
                    scope.frame_length,
                    &scope.class_tag,
                    &scope.paired_class_tag,
                )
                .is_some()
            {
                scope
                    .reference_members
                    .get(ordinal + 1)
                    .and_then(|record_index| headers.get(&(stream, *record_index)))
                    .map(|header| header.byte_offset)
            } else {
                None
            };
            if let Some(operand) = parse_face_operand(
                bytes,
                records,
                scope,
                scope_reference_ordinal,
                None,
                next_byte_offset,
                header,
                recipes,
            ) {
                out.push(operand);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode the ordered persistent source identities carried by admitted `Face`
/// source envelopes. The source envelope is distinct from a face-regeneration
/// recipe: its members can name curves and vertices in one operation.
pub fn decode_face_source_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignFaceSourceGroup>, CodecError> {
    let mut out = Vec::new();
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    for scope in scopes.iter().filter(|scope| scope.kind == "Face") {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        let Ok(scope_start) = usize::try_from(scope.byte_offset) else {
            continue;
        };
        let mut reference_headers = Vec::with_capacity(scope.reference_members.len());
        for record_index in &scope.reference_members {
            let Some(byte_offset) = records.first_at_or_after(
                scope_start.saturating_add(indexed_header::LEN),
                *record_index,
            ) else {
                reference_headers.push(None);
                continue;
            };
            let Some((class_tag, _)) =
                lp_ascii_filtered(bytes, byte_offset, 3..=3, u8::is_ascii_digit)
            else {
                reference_headers.push(None);
                continue;
            };
            reference_headers.push(Some((*record_index, byte_offset, class_tag)));
        }
        for (carrier_ordinal, carrier) in reference_headers.iter().enumerate() {
            let Some((carrier_record_index, carrier_byte_offset, carrier_class_tag)) = carrier
            else {
                continue;
            };
            let Some(layout) = face_source_carrier_layout(carrier_class_tag) else {
                continue;
            };
            let Some((paired_record_index, paired_byte_offset, paired_class_tag)) =
                reference_headers
                    .get(carrier_ordinal + 1)
                    .and_then(Option::as_ref)
            else {
                continue;
            };
            if paired_class_tag != layout.paired_class_tag {
                continue;
            }
            let Some(source_reference_offsets) = parse_face_source_carrier_prefix(
                bytes,
                *carrier_byte_offset,
                scope.record_index,
                layout,
            ) else {
                continue;
            };
            let mut source_members = Vec::with_capacity(source_reference_offsets.len());
            for (_, source_record_index) in &source_reference_offsets {
                let Some(source_byte_offset) = records.first_at_or_after(
                    carrier_byte_offset.saturating_add(indexed_header::LEN),
                    *source_record_index,
                ) else {
                    source_members.clear();
                    break;
                };
                let Some((source_class_tag, _)) =
                    lp_ascii_filtered(bytes, source_byte_offset, 3..=3, u8::is_ascii_digit)
                else {
                    source_members.clear();
                    break;
                };
                let Some(member) = parse_extrude_identity_member(bytes, source_byte_offset) else {
                    source_members.clear();
                    break;
                };
                let Ok(source_byte_offset_u64) = u64::try_from(source_byte_offset) else {
                    source_members.clear();
                    break;
                };
                source_members.push(DesignFaceSourceMember {
                    record_index: *source_record_index,
                    byte_offset: source_byte_offset_u64,
                    class_tag: source_class_tag,
                    persistent_identity: DesignConstructionPersistentIdentity {
                        local_id: member.local_id,
                        local_id_offset: member.local_id_offset,
                        asset_id: member.asset_id,
                        asset_id_offset: member.asset_id_offset,
                        context_id: member.context_id,
                        context_id_offset: member.context_id_offset,
                        tail_slot_present: member.tail_slot_present,
                        tail_slot_offset: member.tail_slot_offset,
                        next_record_index: member.next_record_index,
                        next_byte_offset: member.next_byte_offset,
                    },
                });
            }
            if source_members.len() != source_reference_offsets.len() {
                continue;
            }
            let Ok(carrier_reference_ordinal) = u32::try_from(carrier_ordinal) else {
                continue;
            };
            let (Ok(carrier_byte_offset_u64), Ok(carrier_frame_length), Ok(paired_byte_offset_u64)) = (
                u64::try_from(*carrier_byte_offset),
                u64::try_from(paired_byte_offset.saturating_sub(*carrier_byte_offset)),
                u64::try_from(*paired_byte_offset),
            ) else {
                continue;
            };
            let Ok(source_reference_offsets) = source_reference_offsets
                .iter()
                .map(|(offset, _)| u64::try_from(*offset))
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            out.push(DesignFaceSourceGroup {
                id: ids::native_design_face_source_group_id(&entry.name, *carrier_byte_offset),
                scope_record_index: scope.record_index,
                carrier_reference_ordinal,
                carrier_record_index: *carrier_record_index,
                carrier_byte_offset: carrier_byte_offset_u64,
                carrier_class_tag: carrier_class_tag.clone(),
                carrier_frame_length,
                paired_record_index: *paired_record_index,
                paired_byte_offset: paired_byte_offset_u64,
                paired_class_tag: paired_class_tag.clone(),
                source_reference_offsets,
                source_members,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[derive(Clone, Copy)]
struct FaceSourceCarrierLayout {
    source_count: usize,
    source_reference_offset: usize,
    scalar_offset: usize,
    scalar_discriminator: u32,
    paired_class_tag: &'static str,
}

fn face_source_carrier_layout(class_tag: &str) -> Option<FaceSourceCarrierLayout> {
    match class_tag {
        "398" => Some(FaceSourceCarrierLayout {
            source_count: 4,
            source_reference_offset: 36,
            scalar_offset: 80,
            scalar_discriminator: 100,
            paired_class_tag: "462",
        }),
        "394" => Some(FaceSourceCarrierLayout {
            source_count: 2,
            source_reference_offset: 36,
            scalar_offset: 58,
            scalar_discriminator: 109,
            paired_class_tag: "311",
        }),
        "356" => Some(FaceSourceCarrierLayout {
            source_count: 2,
            source_reference_offset: 36,
            scalar_offset: 58,
            scalar_discriminator: 109,
            paired_class_tag: "309",
        }),
        _ => None,
    }
}

pub(crate) fn face_source_carrier_spec(
    class_tag: &str,
    paired_class_tag: &str,
) -> Option<(usize, usize, usize, u32)> {
    let layout = face_source_carrier_layout(class_tag)?;
    (layout.paired_class_tag == paired_class_tag).then_some((
        layout.source_count,
        layout.source_reference_offset,
        layout.scalar_offset,
        layout.scalar_discriminator,
    ))
}

fn parse_face_source_carrier_prefix(
    bytes: &[u8],
    start: usize,
    scope_record_index: u32,
    layout: FaceSourceCarrierLayout,
) -> Option<Vec<(usize, u32)>> {
    if bytes
        .get(start + indexed_header::LEN..start + indexed_header::LEN + 10)
        .is_none_or(|lane| lane != [0; 10])
        || marked_face_source_reference(bytes, start + 21)? != scope_record_index
        || View::u32_le_at(bytes, start + 32)? != u32::try_from(layout.source_count).ok()?
        || View::u32_le_at(bytes, start + layout.scalar_offset)? != layout.scalar_discriminator
        || !View::f64_le_at(bytes, start + layout.scalar_offset + 4)?.is_finite()
        || View::u32_le_at(bytes, start + layout.scalar_offset + 12)? != layout.scalar_discriminator
    {
        return None;
    }
    (0..layout.source_count)
        .map(|ordinal| {
            let offset = start
                .checked_add(layout.source_reference_offset)?
                .checked_add(ordinal.checked_mul(11)?)?;
            Some((offset, marked_face_source_reference(bytes, offset)?))
        })
        .collect()
}

fn marked_face_source_reference(bytes: &[u8], offset: usize) -> Option<u32> {
    (bytes.get(offset) == Some(&1))
        .then(|| {
            bytes
                .get(offset + 5..offset + 11)
                .is_some_and(|tail| tail == [0; 6])
                .then(|| View::u32_le_at(bytes, offset + 1))
                .flatten()
        })
        .flatten()
}

/// Join each face recipe's persistent Design reference to active solved faces.
pub fn bind_face_operand_candidates(
    operands: &mut [DesignFaceOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    for operand in operands {
        operand.alternate_selector_candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces = tags
            .iter()
            .filter(|tag| {
                crate::ids::same_native_occurrence(&tag.id, &operand.id)
                    && tag.design_references.contains(&design_reference)
            })
            .filter_map(|tag| match &tag.target {
                AttributeTarget::Face(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        operand
            .candidate_faces
            .sort_by(|left, right| left.0.cmp(&right.0));
        operand.candidate_faces.dedup();
        let referenced = operand
            .recipe_references
            .iter()
            .filter(|reference| reference.design_reference == design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<HashSet<_>>();
        operand.unreferenced_candidate_faces = operand
            .candidate_faces
            .iter()
            .filter(|face| !referenced.contains(face))
            .cloned()
            .collect();
        operand.alternate_selector_candidate_faces = operand
            .recipe_references
            .iter()
            .filter(|reference| reference.design_reference == design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .cloned()
            .collect();
        operand
            .alternate_selector_candidate_faces
            .sort_by(|left, right| left.0.cmp(&right.0));
        operand.alternate_selector_candidate_faces.dedup();
    }
}

/// Join each edge recipe's persistent Design reference to active solved faces.
pub fn bind_edge_operand_candidates(
    operands: &mut [DesignEdgeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
) {
    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    for operand in operands {
        operand.candidate_faces.clear();
        for reference in &mut operand.recipe_references {
            bind_recipe_reference_candidates(reference, tags, Some(&operand.id));
        }
        let Some(design_reference) = recipes
            .get(operand.recipe_id.as_str())
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
        else {
            continue;
        };
        operand.candidate_faces =
            edge_operand_candidate_faces(design_reference, tags, Some(&operand.id));
    }
}

pub(crate) fn edge_operand_candidate_faces(
    design_reference: i64,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            owner_id.is_none_or(|owner_id| crate::ids::same_native_occurrence(&tag.id, owner_id))
                && tag.design_references.contains(&design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(id) => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

/// Resolve the unique sketch-profile frame named by profile-based scopes.
pub fn bind_sketch_profiles(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sweep)
            || scope.kind == "BaseFlange"
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let candidates = scope
            .reference_members
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(ordinal, record_index)| {
                let ordinal = u32::try_from(ordinal).ok()?;
                let header = headers.get(&(stream, record_index))?;
                parse_sketch_profile(bytes, stream, ordinal, header, entities)
            })
            .collect::<Vec<_>>();
        if let [profile] = candidates.as_slice() {
            if scope.kind == "BaseFlange" {
                scope.ensure_base_flange().base_flange_profile = Some(profile.clone());
            } else if design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sweep) {
                scope.ensure_path_feature().sweep_profile = Some(profile.clone());
            } else {
                scope.ensure_extrude().extrude_profile = Some(profile.clone());
            }
        }
    }
    Ok(())
}

/// Decode the counted selection group named by each Extrude scope.
pub fn decode_extrude_selection_groups(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionGroup>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut group) = parse_extrude_selection_group(bytes, scope, ordinal, header) {
                group.id =
                    ids::native_design_extrude_selection_group_id(&entry.name, header.byte_offset);
                out.push(group);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode counted construction-operand groups named by feature scopes.
///
/// A scope reference member whose record opens the group grammar but does not
/// close it is recorded on the owning scope, so a group the grammar cannot read
/// is distinguishable from a reference member that is not a group at all.
pub fn decode_construction_operand_groups(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandGroup>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|h| Some(((native_stream(&h.id)?, h.record_index), h)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Coil)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Loft)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Sweep)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Pipe)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::OffsetFaces)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Revolve)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Shell)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Thicken)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Move)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::SurfacePatch)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::SurfaceRuled)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::BoundaryFill)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Split)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Draft)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::ReplaceFace)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::SurfaceOffset)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::SurfaceTrim)
            || scope.kind == "SplitFace"
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Scale)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::CircularPattern)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::RectangularPattern)
            || design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Mirror)
            || scope.kind == "RemoveBody"
            || scope.kind == "SurfaceStitch"
            || scope.kind == "DeleteFace"
            || scope.kind == "SurfaceDeleteFace"
            || scope.kind == "Decal"
            || scope.kind == "Thread"
            || scope.kind == "Hole"
            || matches!(scope.kind.as_str(), "BaseFlange" | "EdgeFlange" | "Hem")
            || has_typed_edge_treatment_group(&scope.kind)
    }) {
        let scope_group_start = out.len();
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut unclosed = Vec::new();
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            match parse_construction_operand_group(bytes, scope, ordinal, header) {
                ConstructionOperandGroupParse::Complete(mut group) => {
                    group.id = ids::native_design_construction_operand_group_id(
                        &entry.name,
                        header.byte_offset,
                    );
                    out.push(*group);
                }
                ConstructionOperandGroupParse::Unclosed => unclosed.push(record_index),
                ConstructionOperandGroupParse::NotAGroup => {}
            }
        }
        scope.unclosed_construction_operand_groups = unclosed;
        if design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Extrude) {
            assign_extrude_face_roles(scope, &mut out[scope_group_start..]);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode the fixed role-less body carrier used by the legacy Boolean-Loft
/// envelopes. The ordinary role-`0x8` body group is admitted only when this
/// exact carrier is present at scope-reference ordinal zero.
pub fn decode_loft_legacy_body_carriers(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignLoftLegacyBodyCarrier>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes.iter().filter(|scope| {
        design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Loft)
            && matches!(
                scope.path_feature_construction(),
                Some(DesignPathFeatureConstruction::Loft { operation, .. })
                    if *operation != crate::records::DesignExtrudeOperation::NewBody
            )
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in scope.reference_members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            if ordinal != 0 {
                continue;
            }
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Some(mut carrier) = parse_loft_legacy_body_carrier(bytes, scope, header) else {
                continue;
            };
            carrier.id =
                ids::native_design_loft_legacy_body_carrier_id(&entry.name, header.byte_offset);
            out.push(carrier);
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    out.dedup_by(|left, right| left.id == right.id);
    Ok(out)
}

/// Parse one class-`322`/`262` or class-`411`/`266` legacy Loft body carrier.
pub(crate) fn parse_loft_legacy_body_carrier(
    bytes: &[u8],
    scope: &DesignParameterScope,
    header: &DesignRecordHeader,
) -> Option<DesignLoftLegacyBodyCarrier> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let (paired_class, frame_length, has_trailing_scope) = match header.class_tag.as_str() {
        "322" => {
            let short_paired_offset = start.checked_add(legacy_loft_322::LEN)?;
            let long_paired_offset = start.checked_add(legacy_loft_322_tail::LEN)?;
            let short_pair_matches = indexed_record_index(bytes, short_paired_offset)
                == Some(header.record_index)
                && bytes
                    .get(
                        short_paired_offset + indexed_header::CLASS_TAG
                            ..short_paired_offset + indexed_header::CLASS_TAG + 3,
                    )
                    .is_some_and(|class_tag| class_tag == b"262");
            let long_pair_matches = indexed_record_index(bytes, long_paired_offset)
                == Some(header.record_index)
                && bytes
                    .get(
                        long_paired_offset + indexed_header::CLASS_TAG
                            ..long_paired_offset + indexed_header::CLASS_TAG + 3,
                    )
                    .is_some_and(|class_tag| class_tag == b"262");
            if short_pair_matches {
                ("262", legacy_loft_322::LEN, false)
            } else if long_pair_matches {
                ("262", legacy_loft_322_tail::LEN, true)
            } else {
                return None;
            }
        }
        "411" => ("266", legacy_loft_411::LEN, true),
        _ => return None,
    };
    if indexed_record_index(bytes, start) != Some(header.record_index)
        || bytes
            .get(start + legacy_loft_322::ZERO_RUN_10..start + legacy_loft_322::ZERO_RUN_10 + 10)?
            != [0; 10]
        || bytes.get(start + legacy_loft_322::PRESENCE)? != &1
        || View::u32_le_at(bytes, start + legacy_loft_322::OWNER_SCOPE_RECORD_INDEX)?
            != scope.record_index
        || bytes
            .get(start + legacy_loft_322::ZERO_RUN_6..start + legacy_loft_322::ZERO_RUN_6 + 6)?
            != [0; 6]
        || View::u32_le_at(bytes, start + legacy_loft_322::MEMBER_COUNT)? != 1
    {
        return None;
    }
    let mut cursor = start.checked_add(legacy_loft_322::MEMBER_REFERENCE)?;
    let member_offset = cursor;
    let (member, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::OPAQUE_INDEX)? {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, cursor)?;
    if opaque_index == 0 || opaque_index >= 256 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let opaque_scalar = View::f64_le_at(bytes, cursor)?;
    if !opaque_scalar.is_finite() {
        return None;
    }
    cursor = cursor.checked_add(8)?;
    let repeated_opaque_index = View::u32_le_at(bytes, cursor)?;
    if repeated_opaque_index != opaque_index {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    let (next_next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::FLAGS)?
        || bytes.get(cursor..cursor + 2)? != [0, 0]
    {
        return None;
    }
    let flags: [u8; 2] = bytes.get(cursor..cursor + 2)?.try_into().ok()?;
    cursor = cursor.checked_add(2)?;
    let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
    if cursor != start.checked_add(legacy_loft_322::LEN)? {
        return None;
    }
    let (trailing_scope_record_index, trailing_scope_reference_offset) = if has_trailing_scope {
        if cursor != start + legacy_loft_322_tail::TAIL_ZERO || bytes.get(cursor)? != &0 {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        if cursor != start + legacy_loft_322_tail::TRAILING_SCOPE_REFERENCE {
            return None;
        }
        let reference_offset = cursor;
        let (record_index, _) = take_record_reference(bytes, &mut cursor)?;
        if record_index != scope.record_index {
            return None;
        }
        (
            Some(record_index),
            Some(u64::try_from(reference_offset).ok()?),
        )
    } else {
        (None, None)
    };
    let paired_byte_offset = start.checked_add(frame_length)?;
    if cursor != paired_byte_offset
        || indexed_record_index(bytes, paired_byte_offset) != Some(header.record_index)
    {
        return None;
    }
    let paired_class_tag = std::str::from_utf8(bytes.get(
        paired_byte_offset + indexed_header::CLASS_TAG
            ..paired_byte_offset + indexed_header::CLASS_TAG + 3,
    )?)
    .ok()?;
    if paired_class_tag != paired_class {
        return None;
    }
    Some(DesignLoftLegacyBodyCarrier {
        id: String::new(),
        scope_record_index: scope.record_index,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        owner_scope_record_index: scope.record_index,
        owner_scope_record_index_offset: u64::try_from(
            start + legacy_loft_322::OWNER_SCOPE_RECORD_INDEX,
        )
        .ok()?,
        member,
        member_offset: u64::try_from(member_offset).ok()?,
        member_count_offset: u64::try_from(start + legacy_loft_322::MEMBER_COUNT).ok()?,
        opaque_index,
        opaque_index_offset: u64::try_from(start + legacy_loft_322::OPAQUE_INDEX).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(start + legacy_loft_322::OPAQUE_SCALAR).ok()?,
        repeated_opaque_index,
        repeated_opaque_index_offset: u64::try_from(start + legacy_loft_322::REPEATED_OPAQUE_INDEX)
            .ok()?,
        next_next_record_index,
        next_next_reference_offset: u64::try_from(start + legacy_loft_322::NEXT_NEXT_REFERENCE)
            .ok()?,
        flags,
        flags_offset: u64::try_from(start + legacy_loft_322::FLAGS).ok()?,
        next_record_index,
        next_reference_offset: u64::try_from(start + legacy_loft_322::NEXT_REFERENCE).ok()?,
        trailing_scope_record_index,
        trailing_scope_reference_offset,
        paired_class_tag: paired_class_tag.to_owned(),
        paired_byte_offset: u64::try_from(paired_byte_offset).ok()?,
    })
}

pub(crate) fn assign_extrude_face_roles(
    scope: &DesignParameterScope,
    groups: &mut [DesignConstructionOperandGroup],
) {
    let mut face_groups = groups.iter_mut().filter(|group| {
        group
            .extrude_role
            .is_some_and(|role| matches!(role, DesignExtrudeOperandRole::Faces(_)))
    });
    if scope.extrude_prologue().map(DesignExtrudePrologue::start)
        == Some(DesignExtrudeStart::FromFace)
    {
        if let Some(group) = face_groups.next() {
            group.extrude_role = Some(DesignExtrudeOperandRole::Faces(Some(
                DesignExtrudeFaceRole::Start,
            )));
        }
    }
    for group in face_groups {
        group.extrude_role = Some(DesignExtrudeOperandRole::Faces(Some(
            DesignExtrudeFaceRole::Termination,
        )));
    }
}

/// Pair Fillet construction-operand groups with their radius inputs.
pub fn decode_fillet_radius_groups(
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Vec<DesignFilletRadiusGroup> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Fillet))
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let mut scope_groups = groups
            .iter()
            .filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
        let mut owned_parameters = owners
            .iter()
            .filter(|owner| {
                native_stream(&owner.id) == Some(stream)
                    && owner.scope_record_index == scope.record_index
            })
            .filter_map(|owner| {
                Some((
                    owner.local_ordinal,
                    *parameters.get(&(stream, owner.parameter_record_index))?,
                ))
            })
            .collect::<Vec<_>>();
        owned_parameters.sort_by_key(|(ordinal, _)| *ordinal);
        let radii = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind == "Radius").then_some(*parameter))
            .collect::<Vec<_>>();
        let weights = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind == "TangencyWeight").then_some(*parameter)
            })
            .collect::<Vec<_>>();
        if owned_parameters.len() == radii.len() + weights.len()
            && scope_groups.len() == radii.len()
            && (weights.is_empty() || weights.len() == scope_groups.len())
        {
            for (ordinal, (group, radius)) in scope_groups.into_iter().zip(radii).enumerate() {
                let Ok(group_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                out.push(DesignFilletRadiusGroup {
                    id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                    scope_record_index: scope.record_index,
                    group_ordinal,
                    group_record_index: group.record_index,
                    edge_operand_record_indices: group.members.clone(),
                    law: DesignFilletRadiusLaw::Constant {
                        radius_parameter_record_index: radius.record_index,
                    },
                    tangency_weight_parameter_record_index: weights
                        .get(ordinal)
                        .map(|parameter| parameter.record_index),
                });
            }
            continue;
        }
        let [group] = scope_groups.as_slice() else {
            continue;
        };
        let chord_lengths = owned_parameters
            .iter()
            .filter_map(|(_, parameter)| {
                (parameter.source_kind == "ChordLen").then_some(parameter.record_index)
            })
            .collect::<Vec<_>>();
        // TangencyWeight is optional for the chordal law; older records carry
        // only the required ChordLen input.
        if (weights.is_empty() && owned_parameters.len() == 1)
            || (weights.len() == 1 && owned_parameters.len() == 2)
        {
            let [chord_length] = chord_lengths.as_slice() else {
                continue;
            };
            out.push(DesignFilletRadiusGroup {
                id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                scope_record_index: scope.record_index,
                group_ordinal: 0,
                group_record_index: group.record_index,
                edge_operand_record_indices: group.members.clone(),
                law: DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index: *chord_length,
                },
                tangency_weight_parameter_record_index: weights
                    .first()
                    .map(|parameter| parameter.record_index),
            });
            continue;
        }
        let asymmetric_offsets = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind == kind).then_some(parameter.record_index)
                })
                .collect::<Vec<_>>()
        };
        let (offset_one, offset_two) = (
            asymmetric_offsets("EdgeOffset1"),
            asymmetric_offsets("EdgeOffset2"),
        );
        if owned_parameters.len() == 3 {
            if let ([offset_one], [offset_two], [weight]) = (
                offset_one.as_slice(),
                offset_two.as_slice(),
                weights.as_slice(),
            ) {
                out.push(DesignFilletRadiusGroup {
                    id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
                    scope_record_index: scope.record_index,
                    group_ordinal: 0,
                    group_record_index: group.record_index,
                    edge_operand_record_indices: group.members.clone(),
                    law: DesignFilletRadiusLaw::Asymmetric {
                        offset_one_parameter_record_index: *offset_one,
                        offset_two_parameter_record_index: *offset_two,
                    },
                    tangency_weight_parameter_record_index: Some(weight.record_index),
                });
                continue;
            }
        }
        let records = |kind: &str| {
            owned_parameters
                .iter()
                .filter_map(|(_, parameter)| {
                    (parameter.source_kind == kind).then_some(parameter.record_index)
                })
                .collect::<Vec<_>>()
        };
        let (start, end, middle_radii, middle_parameters) = (
            records("StartRadius"),
            records("EndRadius"),
            records("MidRadius"),
            records("MidParams"),
        );
        let ([start], [end]) = (start.as_slice(), end.as_slice()) else {
            continue;
        };
        // TangencyWeight is optional for the variable-radius law. Older
        // records carry only the endpoint and midpoint radius parameters.
        let variable_parameter_count =
            2 + middle_radii.len() + middle_parameters.len() + weights.len();
        if middle_radii.len() != middle_parameters.len()
            || weights.len() > 1
            || owned_parameters.len() != variable_parameter_count
        {
            continue;
        }
        out.push(DesignFilletRadiusGroup {
            id: format!("{stream}:design-fillet-radius-group#{}", group.record_index),
            scope_record_index: scope.record_index,
            group_ordinal: 0,
            group_record_index: group.record_index,
            edge_operand_record_indices: group.members.clone(),
            law: DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: *start,
                end_radius_parameter_record_index: *end,
                middle_radius_parameter_record_indices: middle_radii,
                middle_parameter_record_indices: middle_parameters,
            },
            tangency_weight_parameter_record_index: weights
                .first()
                .map(|parameter| parameter.record_index),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Remove fixed Fillet interpretations of frames that are indexed parameter owners.
pub fn disambiguate_fixed_fillet_parameters(
    scopes: &mut [DesignParameterScope],
    owners: &[DesignParameterOwner],
) {
    let indexed_scopes = owners
        .iter()
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.scope_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if indexed_scopes.contains(&(stream.to_owned(), scope.record_index)) {
            scope.set_fixed_fillet_parameters(None);
        }
    }
}

/// Outcome of reading a scope reference member as a construction-operand group.
pub(crate) enum ConstructionOperandGroupParse {
    /// The record does not open a construction-operand group.
    NotAGroup,
    /// The record opens a group the grammar does not close.
    Unclosed,
    /// A complete group.
    Complete(Box<DesignConstructionOperandGroup>),
}

impl ConstructionOperandGroupParse {
    /// The group, where the record carried a complete one.
    #[cfg(test)]
    pub(crate) fn complete(self) -> Option<DesignConstructionOperandGroup> {
        match self {
            Self::Complete(group) => Some(*group),
            Self::NotAGroup | Self::Unclosed => None,
        }
    }
}

/// Interpret the role of a counted group owned by an Extrude scope.
///
/// The `0x12` face-group role is a legacy spelling of the one-sided-to-face
/// termination group. Class-296 two-sided-to-faces scopes use the same role
/// for their termination groups. It is also a valid Thicken role, so the
/// extent and exact-layout gates are part of this admission rule rather than
/// a global role alias.
fn extrude_operand_role(
    scope: &DesignParameterScope,
    role: u64,
) -> Option<DesignExtrudeOperandRole> {
    if design_feature_family(&scope.kind) != Some(DesignFeatureFamily::Extrude) {
        return None;
    }
    match role {
        0x0000_0004_0000_0000 | 0x0000_0008_0000_0000 => Some(DesignExtrudeOperandRole::Bodies),
        0x0000_0041_0000_0000 => Some(DesignExtrudeOperandRole::Profile),
        0x0000_0011_0000_0000 => Some(DesignExtrudeOperandRole::Faces(None)),
        0x0000_0005_0000_0000
            if scope.extrude_prologue().map(DesignExtrudePrologue::start)
                == Some(DesignExtrudeStart::FromFace) =>
        {
            Some(DesignExtrudeOperandRole::Faces(None))
        }
        0x0000_0012_0000_0000
            if scope
                .extrude_prologue()
                .and_then(DesignExtrudePrologue::extent)
                == Some(DesignExtrudeExtent::OneSidedToFace) =>
        {
            Some(DesignExtrudeOperandRole::Faces(None))
        }
        0x0000_0012_0000_0000 if is_class_296_two_sided_to_faces_scope(scope) => {
            Some(DesignExtrudeOperandRole::Faces(None))
        }
        _ => None,
    }
}

/// Read the construction-operand group at `header`.
///
/// The record's members are a leading-block presence byte, the property block
/// its presence byte gates, the counted member run, two optional references,
/// the counted trailing-reference run, a zero u32 and the u32 role, ten zero bytes, an
/// ordinal, a duration, and a repeat of the ordinal that one container
/// generation omits. The class-328 Move form has one null and one present
/// auxiliary reference, a zero trailing count, and a retained null trailing
/// slot before the role. The ordinary tail is a reference to record `N + 2`,
/// a flag block, a reference to record `N + 1`, a zero byte, the owning scope's
/// reference, and the same-index paired header. The class-328 Move tail has a
/// leading zero, the flag block, an unmarked `N + 1` u64 reference with three
/// zero bytes, and the owning-scope reference. The flag block's last byte is
/// zero; one container generation prefixes it with a further byte. Neither
/// the repeated ordinal nor the prefix byte is announced, so the tail settles
/// both: exactly one of the four readings reaches a paired header carrying
/// this record's own index.
pub(crate) fn parse_construction_operand_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> ConstructionOperandGroupParse {
    use ConstructionOperandGroupParse::{Complete, NotAGroup, Unclosed};

    let Ok(start) = usize::try_from(header.byte_offset) else {
        return NotAGroup;
    };
    // The indexed header is the three-digit class tag, the u64 entity id whose
    // low word is the record index, and the record's own empty name.
    if bytes.get(start + 11..start + 19) != Some(&[0; 8]) {
        return NotAGroup;
    }
    let Some(mut cursor) = payload_prologue(bytes, start + 19, bytes.len()) else {
        return NotAGroup;
    };
    let member_count_at = cursor;
    let Some(member_count) = View::u32_le_at(bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    // A reference is at least one byte, so a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if usize::try_from(member_count).unwrap_or(usize::MAX) > bytes.len().saturating_sub(cursor) {
        return NotAGroup;
    }
    let mut members = Vec::new();
    let mut member_offsets = Vec::new();
    for _ in 0..member_count {
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        members.push(record_index);
        member_offsets.push(offset);
    }
    let mut auxiliary_record_indices = Vec::new();
    let mut auxiliary_record_offsets = Vec::new();
    let mut auxiliary_reference_slots = [false; 2];
    for present in &mut auxiliary_reference_slots {
        if bytes.get(cursor) == Some(&0) {
            cursor += 1;
            continue;
        }
        *present = true;
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        auxiliary_record_indices.push(record_index);
        auxiliary_record_offsets.push(offset);
    }
    let Some(trailing_count) = View::u32_le_at(bytes, cursor) else {
        return NotAGroup;
    };
    cursor += 4;
    if usize::try_from(trailing_count).unwrap_or(usize::MAX) > bytes.len().saturating_sub(cursor) {
        return NotAGroup;
    }
    let mut trailing_record_indices = Vec::new();
    let mut trailing_record_offsets = Vec::new();
    for _ in 0..trailing_count {
        let Some((record_index, offset)) = take_record_reference(bytes, &mut cursor) else {
            return NotAGroup;
        };
        trailing_record_indices.push(record_index);
        trailing_record_offsets.push(offset);
    }
    let legacy_move_class_328 = scope.kind == "Move"
        && header.class_tag == "328"
        && auxiliary_reference_slots == [false, true]
        && header
            .record_index
            .checked_add(13)
            .is_some_and(|expected| auxiliary_record_indices.as_slice() == [expected])
        && trailing_count == 0;
    if legacy_move_class_328 {
        if bytes.get(cursor) != Some(&0) {
            return NotAGroup;
        }
        cursor += 1;
    }
    // The role occupies the high word of a u64 whose low word is zero.
    let role_at = cursor;
    let (Some(0), Some(role)) = (
        View::u32_le_at(bytes, role_at),
        View::u64_le_at(bytes, role_at),
    ) else {
        return NotAGroup;
    };
    cursor += 8;
    if bytes.get(cursor..cursor + 10) != Some(&[0; 10]) {
        return NotAGroup;
    }
    cursor += 10;
    let opaque_index_at = cursor;
    let (Some(opaque_index), Some(opaque_scalar)) = (
        View::u32_le_at(bytes, cursor),
        View::f64_le_at(bytes, cursor + 4),
    ) else {
        return NotAGroup;
    };
    if opaque_index == 0 || !opaque_scalar.is_finite() {
        return NotAGroup;
    }
    cursor += 12;

    // Past this point the record has opened the group grammar, so a tail that
    // does not close is a group this reader cannot name.
    let mut closed = None;
    for repeats_ordinal in [true, false] {
        let mut tail = cursor;
        if repeats_ordinal {
            if View::u32_le_at(bytes, tail) != Some(opaque_index) {
                continue;
            }
            tail += 4;
        }
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != header.record_index.checked_add(2)
        {
            continue;
        }
        for flag_bytes in [2usize, 3] {
            let Some(flags) = bytes.get(tail..tail + flag_bytes) else {
                continue;
            };
            if flags.last() != Some(&0) {
                continue;
            }
            // The wider block prefixes the narrower one, so the variant flag is
            // always the byte before the terminating zero.
            let variant = flags[flag_bytes - 2] != 0;
            let mut after = tail + flag_bytes;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != header.record_index.checked_add(1)
            {
                continue;
            }
            if bytes.get(after) != Some(&0) {
                continue;
            }
            after += 1;
            if take_record_reference(bytes, &mut after).map(|(index, _)| index)
                != Some(scope.record_index)
            {
                continue;
            }
            let Some((paired_class_tag, after_tag)) =
                lp_ascii_filtered(bytes, after, 3..=3, u8::is_ascii_digit)
            else {
                continue;
            };
            if View::u32_le_at(bytes, after_tag) != Some(header.record_index) {
                continue;
            }
            if closed.replace((variant, after, paired_class_tag)).is_some() {
                return Unclosed;
            }
        }
    }
    if let Some(legacy_tail) = legacy_body_group_tail(bytes, scope, header, cursor, opaque_index) {
        if closed.replace(legacy_tail).is_some() {
            return Unclosed;
        }
    }
    let Some((variant, paired_at, paired_class_tag)) = closed else {
        return Unclosed;
    };

    let extrude_role = extrude_operand_role(scope, role);
    let (Ok(member_count_offset), Ok(role_offset), Ok(opaque_index_offset), Ok(paired_byte_offset)) = (
        u64::try_from(member_count_at),
        u64::try_from(role_at),
        u64::try_from(opaque_index_at),
        u64::try_from(paired_at),
    ) else {
        return Unclosed;
    };
    Complete(Box::new(DesignConstructionOperandGroup {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        members,
        lost_edge_references: Vec::new(),
        member_offsets,
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset,
            auxiliary_record_indices,
            auxiliary_record_offsets,
            auxiliary_paths: Vec::new(),
            trailing_record_indices,
            trailing_record_offsets,
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index,
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: opaque_index_offset + 4,
            variant,
        },
        role,
        extrude_role,
        role_offset,
        paired_class_tag,
        paired_byte_offset,
    }))
}

/// Read the legacy Move/RemoveBody tail whose two flag bytes have no
/// terminating zero. The class and feature gates keep this admission separate
/// from the terminated flag-block grammar used by other construction groups.
fn legacy_body_group_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    header: &DesignRecordHeader,
    cursor: usize,
    opaque_index: u32,
) -> Option<(bool, usize, String)> {
    let body_scope = design_feature_family(&scope.kind) == Some(DesignFeatureFamily::Move)
        || scope.kind == "RemoveBody";
    let (flag_pair, variant) = match header.class_tag.as_str() {
        "257" | "323" | "338" if body_scope => ([1, 1], true),
        "328" if scope.kind == "Move" => ([1, 1], true),
        "282" | "302" if body_scope => ([0, 1], false),
        _ => return None,
    };
    let mut tail = cursor;
    if View::u32_le_at(bytes, tail)? != opaque_index {
        return None;
    }
    tail += 4;
    if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
        != header.record_index.checked_add(2)
    {
        return None;
    }
    if scope.kind == "Move" && header.class_tag == "328" {
        if bytes.get(tail) != Some(&0) {
            return None;
        }
        tail += 1;
        if bytes.get(tail..tail + 2) != Some(&flag_pair) {
            return None;
        }
        tail += 2;
        if View::u64_le_at(bytes, tail)? != u64::from(header.record_index.checked_add(1)?)
            || bytes.get(tail + 8..tail + 11) != Some(&[0; 3])
        {
            return None;
        }
        tail += 11;
    } else {
        if bytes.get(tail..tail + 2) != Some(&flag_pair) {
            return None;
        }
        tail += 2;
        if take_record_reference(bytes, &mut tail).map(|(index, _)| index)
            != header.record_index.checked_add(1)
        {
            return None;
        }
        if bytes.get(tail) != Some(&0) {
            return None;
        }
        tail += 1;
    }
    if take_record_reference(bytes, &mut tail).map(|(index, _)| index) != Some(scope.record_index) {
        return None;
    }
    let (paired_class_tag, after_tag) = lp_ascii_filtered(bytes, tail, 3..=3, u8::is_ascii_digit)?;
    if scope.kind == "Move" && header.class_tag == "328" && paired_class_tag != "263" {
        return None;
    }
    if View::u32_le_at(bytes, after_tag) != Some(header.record_index) {
        return None;
    }
    Some((variant, tail, paired_class_tag))
}

/// Bind exact typed records selected by construction-group trailing runs.
pub fn bind_construction_operand_trailing_records(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for group in groups {
        group.frame.trailing_transforms.clear();
        group.frame.trailing_dual_transforms.clear();
        group.frame.trailing_flags.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for record_index in &group.frame.trailing_record_indices {
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            if let Some(transform) = parse_construction_operand_transform(bytes, header) {
                group.frame.trailing_transforms.push(transform);
            } else if let Some(transform) = parse_construction_operand_dual_transform(bytes, header)
            {
                group.frame.trailing_dual_transforms.push(transform);
            } else if let Some(flag) = parse_construction_operand_flag(bytes, header) {
                group.frame.trailing_flags.push(flag);
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_construction_operand_flag(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandFlag> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || bytes.get(start + 23) != Some(&0)
    {
        return None;
    }
    let value = match bytes.get(start + 22)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(crate::records::DesignConstructionOperandFlag {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        value,
        value_offset: u64::try_from(start + 22).ok()?,
    })
}

/// Bind exact persistent-entity path records selected by construction groups.
pub fn bind_construction_operand_paths(
    scan: &ContainerScan,
    groups: &mut [DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<(), CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    for group in groups {
        group.frame.auxiliary_paths.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for record_index in &group.frame.auxiliary_record_indices {
            let Some(header) = headers.get(&(stream, *record_index)) else {
                continue;
            };
            if let Some(path) =
                parse_construction_operand_path(bytes, group.scope_record_index, header)
            {
                group.frame.auxiliary_paths.push(path);
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_construction_operand_path(
    bytes: &[u8],
    expected_scope_record_index: u32,
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandPath> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] || bytes.get(start + 21) != Some(&1) {
        return None;
    }
    let entity_ref = View::u64_le_at(bytes, start + 22)?;
    let entity_ref_offset = u64::try_from(start + 22).ok()?;
    let (transform, transform_offset, compact_variant, mut cursor) =
        if bytes.get(start + 30..start + 33)? == [0; 3] {
            let transform = rigid_transform_at(bytes, start + 33)?;
            if bytes.get(start + 161) != Some(&0) {
                return None;
            }
            (
                Some(transform),
                Some(u64::try_from(start + 33).ok()?),
                None,
                start + 162,
            )
        } else {
            let variant = match bytes.get(start + 30..start + 34)? {
                [0, 0, variant @ (0 | 1), 0] => *variant != 0,
                _ => return None,
            };
            (None, None, Some(variant), start + 34)
        };
    let (scope_record_index, scope_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if scope_record_index != expected_scope_record_index {
        return None;
    }
    let (nested_record_index, nested_record_index_offset) =
        take_record_reference(bytes, &mut cursor)?;
    if nested_record_index != header.record_index.checked_add(2)?
        || bytes.get(cursor..cursor + 6)? != [0; 6]
    {
        return None;
    }
    let following_at = cursor.checked_add(6)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_tag)?;
    if following_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    Some(crate::records::DesignConstructionOperandPath {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        entity_ref,
        entity_ref_offset,
        transform,
        transform_offset,
        compact_variant,
        scope_record_index,
        scope_record_index_offset,
        nested_record_index,
        nested_record_index_offset,
        following_record_index,
        following_byte_offset: u64::try_from(following_at).ok()?,
        following_class_tag,
    })
}

pub(crate) fn parse_construction_operand_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 22)? != [0; 11]
        || bytes.get(start + 150..start + 152)? != [1, 0]
    {
        return None;
    }
    let transform_at = start.checked_add(22)?;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let following_at = start.checked_add(152)?;
    let (following_class_tag, after_tag) =
        lp_ascii_filtered(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_tag)?;
    if following_record_index != header.record_index.checked_add(1)? {
        return None;
    }
    Some(crate::records::DesignConstructionOperandTransform {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        transform,
        transform_offset: u64::try_from(transform_at).ok()?,
        following_record_index,
        following_byte_offset: u64::try_from(following_at).ok()?,
        following_class_tag,
    })
}

pub(crate) fn parse_construction_operand_dual_transform(
    bytes: &[u8],
    header: &DesignRecordHeader,
) -> Option<crate::records::DesignConstructionOperandDualTransform> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10] || bytes.get(start + 277) != Some(&0) {
        return None;
    }
    let first_at = start.checked_add(21)?;
    let second_at = start.checked_add(149)?;
    Some(crate::records::DesignConstructionOperandDualTransform {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        first_transform: rigid_transform_at(bytes, first_at)?,
        first_transform_offset: u64::try_from(first_at).ok()?,
        second_transform: rigid_transform_at(bytes, second_at)?,
        second_transform_offset: u64::try_from(second_at).ok()?,
    })
}

fn rigid_transform_at(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut transform = [[0.0; 4]; 4];
    for row in &mut transform {
        for cell in row {
            *cell = view.f64_le()?;
        }
    }
    crate::design::decode::sketch::valid_sketch_transform(&transform).then_some(transform)
}

/// Take one reference naming a record of the same segment, advancing `at` past
/// every byte it owns. Returns the record index and the byte offset of the low
/// word of the target entity id.
fn take_record_reference(bytes: &[u8], at: &mut usize) -> Option<(u32, u64)> {
    let target_at = at.checked_add(1)?;
    let reference = take_reference(bytes, at)?;
    if reference.segment.is_some() || reference.link_name.is_some() {
        return None;
    }
    Some((
        u32::try_from(reference.target?).ok()?,
        u64::try_from(target_at).ok()?,
    ))
}

/// Decode the persistent identity frame named by each construction-operand group.
pub fn decode_construction_operand_identities(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignConstructionOperandIdentity>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let Some(trailing_record_index) = group.frame.trailing_record_indices.first() else {
            continue;
        };
        let Some(wrapper_header) = headers.get(&(stream, *trailing_record_index)) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        if let Some(mut identity) =
            parse_construction_operand_identity(bytes, group, wrapper_header)
        {
            identity.id = ids::native_design_construction_operand_identity_id(
                &entry.name,
                wrapper_header.byte_offset,
            );
            out.push(identity);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|left, right| left.id == right.id);
    Ok(out)
}

/// Bind a contiguous unresolved-edge run to the construction group whose
/// first identity wrapper terminates that run.
pub fn bind_lost_edge_groups(
    groups: &mut [DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    lost_edges: &[LostEdgeReference],
) -> Result<(), CodecError> {
    for group in groups {
        group.lost_edge_references.clear();
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let mut identity_matches = identities.iter().filter(|identity| {
            native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == group.record_index
        });
        let Some(identity) = identity_matches.next() else {
            continue;
        };
        if identity_matches.next().is_some() {
            return Err(CodecError::malformed(format_args!(
                "Fusion construction group {} has multiple identity chains",
                group.record_index
            )));
        }
        let Some((wrapper_record_index, wrapper_byte_offset, wrapper_class_tag)) = identity
            .wrapper_record_indices
            .first()
            .zip(identity.wrapper_byte_offsets.first())
            .zip(identity.wrapper_class_tags.first())
            .map(|((record_index, byte_offset), class_tag)| {
                (*record_index, *byte_offset, class_tag.as_str())
            })
        else {
            continue;
        };
        let mut stream_edges = lost_edges
            .iter()
            .filter(|edge| native_stream(&edge.id) == Some(stream))
            .collect::<Vec<_>>();
        stream_edges.sort_by_key(|edge| edge.record_byte_offset);
        let terminals = stream_edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.next_record_index == wrapper_record_index
                    && edge.next_byte_offset == wrapper_byte_offset
                    && edge.next_class_tag == wrapper_class_tag
            })
            .map(|(ordinal, _)| ordinal)
            .collect::<Vec<_>>();
        let [terminal] = terminals.as_slice() else {
            if terminals.is_empty() {
                continue;
            }
            return Err(CodecError::malformed(format_args!(
                "Fusion construction group {} has multiple terminating lost-edge runs",
                group.record_index
            )));
        };
        let mut start = *terminal;
        while start > 0 {
            let previous = stream_edges[start - 1];
            let current = stream_edges[start];
            if previous.next_byte_offset != current.record_byte_offset
                || previous.next_record_index != current.record_index
                || previous.next_class_tag != current.class_tag
            {
                break;
            }
            start -= 1;
        }
        let run = &stream_edges[start..=*terminal];
        if run.len() != group.members.len() {
            return Err(CodecError::malformed(format_args!(
                "Fusion construction group {} has {} operands but its lost-edge run has {} records",
                group.record_index,
                group.members.len(),
                run.len()
            )));
        }
        group.lost_edge_references = run.iter().map(|edge| edge.id.clone()).collect();
    }
    Ok(())
}

pub(crate) fn parse_construction_operand_identity(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    wrapper_header: &DesignRecordHeader,
) -> Option<DesignConstructionOperandIdentity> {
    let mut current_at = usize::try_from(wrapper_header.byte_offset).ok()?;
    let mut current_record_index = wrapper_header.record_index;
    let mut current_class_tag = wrapper_header.class_tag.clone();
    let mut chain_started = false;
    if let Some(transform) = parse_construction_operand_transform(bytes, wrapper_header) {
        current_at = usize::try_from(transform.following_byte_offset).ok()?;
        current_record_index = transform.following_record_index;
        current_class_tag = transform.following_class_tag;
        chain_started = true;
    }
    let mut wrapper_record_indices = Vec::new();
    let mut wrapper_byte_offsets = Vec::new();
    let mut wrapper_class_tags = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if parse_construction_tracking_path(
            bytes,
            current_at,
            current_record_index,
            &current_class_tag,
        )
        .is_some()
            || bytes.get(current_at + 11..current_at + 21)? != [0; 10]
            || bytes.get(current_at + 21..current_at + 24)? != [1, 1, 0]
        {
            break;
        }
        if !seen.insert((current_record_index, current_at)) {
            return None;
        }
        wrapper_record_indices.push(current_record_index);
        wrapper_byte_offsets.push(u64::try_from(current_at).ok()?);
        wrapper_class_tags.push(current_class_tag);
        current_at = current_at.checked_add(24)?;
        let (next_class_tag, after_next_tag) =
            lp_ascii_filtered(bytes, current_at, 0..=2000, u8::is_ascii_graphic)?;
        current_record_index = View::u32_le_at(bytes, after_next_tag)?;
        current_class_tag = next_class_tag;
        chain_started = true;
    }
    let tracking_path = parse_construction_tracking_path(
        bytes,
        current_at,
        current_record_index,
        &current_class_tag,
    );
    if let Some(path) = &tracking_path {
        current_at = usize::try_from(path.following_byte_offset).ok()?;
        current_record_index = path.following_record_index;
        current_class_tag.clone_from(&path.following_class_tag);
        chain_started = true;
    }
    if !chain_started {
        return None;
    }
    let persistent_identity = parse_extrude_identity_member(bytes, current_at).map(|member| {
        DesignConstructionPersistentIdentity {
            local_id: member.local_id,
            local_id_offset: member.local_id_offset,
            asset_id: member.asset_id,
            asset_id_offset: member.asset_id_offset,
            context_id: member.context_id,
            context_id_offset: member.context_id_offset,
            tail_slot_present: member.tail_slot_present,
            tail_slot_offset: member.tail_slot_offset,
            next_record_index: member.next_record_index,
            next_byte_offset: member.next_byte_offset,
        }
    });
    Some(DesignConstructionOperandIdentity {
        id: String::new(),
        group_record_index: group.record_index,
        wrapper_record_indices,
        wrapper_byte_offsets,
        wrapper_class_tags,
        following_record_index: current_record_index,
        following_byte_offset: u64::try_from(current_at).ok()?,
        following_class_tag: current_class_tag,
        tracking_path,
        persistent_identity,
    })
}

pub(crate) fn parse_construction_tracking_path(
    bytes: &[u8],
    wrapper_at: usize,
    wrapper_record_index: u32,
    wrapper_class_tag: &str,
) -> Option<DesignConstructionTrackingPath> {
    if bytes.get(wrapper_at + 11..wrapper_at + 21)? != [0; 10]
        || bytes.get(wrapper_at + 21) != Some(&1)
        || View::u64_le_at(bytes, wrapper_at + 22)?
            != u64::from(wrapper_record_index.checked_add(1)?)
        || bytes.get(wrapper_at + 30..wrapper_at + 33)? != [0; 3]
    {
        return None;
    }
    let carrier_at = wrapper_at.checked_add(33)?;
    let (carrier_class_tag, after_carrier_tag) =
        lp_ascii_filtered(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_record_index = View::u32_le_at(bytes, after_carrier_tag)?;
    if carrier_record_index != wrapper_record_index.checked_add(1)?
        || bytes.get(carrier_at + 11..carrier_at + 21)? != [0; 10]
        || View::u32_le_at(bytes, carrier_at + 21)? != 1
        || View::u32_le_at(bytes, carrier_at + 25)? != 0
        || View::u32_le_at(bytes, carrier_at + 29)? != 1
        || View::u32_le_at(bytes, carrier_at + 33)? != 2
        || View::u64_le_at(bytes, carrier_at + 45)? != 0
        || View::u32_le_at(bytes, carrier_at + 53)? != 1
        || View::u64_le_at(bytes, carrier_at + 65)? != 0
    {
        return None;
    }
    let primary_identity = View::u64_le_at(bytes, carrier_at + 37)?;
    let selector = View::i32_le_at(bytes, carrier_at + 57)?;
    let kind = View::u32_le_at(bytes, carrier_at + 61)?;
    let mut cursor = carrier_at.checked_add(73)?;
    let (first_related_identity, first_related_identity_offset) =
        take_optional_tracking_identity(bytes, &mut cursor)?;
    let (second_related_identity, second_related_identity_offset) =
        take_optional_tracking_identity(bytes, &mut cursor)?;
    let following_at = cursor;
    let (following_class_tag, after_following_tag) =
        lp_ascii_filtered(bytes, following_at, 3..=3, u8::is_ascii_digit)?;
    let following_record_index = View::u32_le_at(bytes, after_following_tag)?;
    if following_record_index != carrier_record_index.checked_add(1)? {
        return None;
    }
    Some(DesignConstructionTrackingPath {
        wrapper_record_index,
        wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
        wrapper_class_tag: wrapper_class_tag.to_owned(),
        carrier_record_index,
        carrier_byte_offset: u64::try_from(carrier_at).ok()?,
        carrier_class_tag,
        primary_identity,
        primary_identity_offset: u64::try_from(carrier_at + 37).ok()?,
        selector,
        selector_offset: u64::try_from(carrier_at + 57).ok()?,
        kind,
        kind_offset: u64::try_from(carrier_at + 61).ok()?,
        first_related_identity,
        first_related_identity_offset,
        second_related_identity,
        second_related_identity_offset,
        following_record_index,
        following_byte_offset: u64::try_from(following_at).ok()?,
        following_class_tag,
    })
}

fn take_optional_tracking_identity(
    bytes: &[u8],
    cursor: &mut usize,
) -> Option<(Option<u64>, Option<u64>)> {
    match View::u32_le_at(bytes, *cursor)? {
        0 => {
            *cursor = (*cursor).checked_add(4)?;
            Some((None, None))
        }
        1 => {
            let value_at = (*cursor).checked_add(4)?;
            let value = View::u64_le_at(bytes, value_at)?;
            *cursor = value_at.checked_add(8)?;
            Some((Some(value), Some(u64::try_from(value_at).ok()?)))
        }
        _ => None,
    }
}

pub(crate) fn parse_extrude_selection_group(
    bytes: &[u8],
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionGroup> {
    let start = usize::try_from(header.byte_offset).ok()?;
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || View::u32_le_at(bytes, start + 22)? != scope.record_index
        || bytes.get(start + 26..start + 32)? != [0; 6]
    {
        return None;
    }
    let member_count = usize::try_from(View::u32_le_at(bytes, start + 32)?).ok()?;
    let mut position = start.checked_add(36)?;
    // Each member consumes 11 bytes; a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if member_count == 0 || member_count > bytes.len().saturating_sub(position) / 11 {
        return None;
    }
    let mut members = Vec::with_capacity(member_count);
    let mut member_offsets = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        if bytes.get(position) != Some(&1) || bytes.get(position + 5..position + 11)? != [0; 6] {
            return None;
        }
        members.push(View::u32_le_at(bytes, position + 1)?);
        member_offsets.push(u64::try_from(position + 1).ok()?);
        position = position.checked_add(11)?;
    }
    let opaque_index = View::u32_le_at(bytes, position)?;
    let opaque_scalar = View::f64_le_at(bytes, position + 4)?;
    if opaque_index == 0
        || !opaque_scalar.is_finite()
        || View::u32_le_at(bytes, position + 12)? != opaque_index
        || bytes.get(position + 16) != Some(&1)
        || View::u32_le_at(bytes, position + 17)? != header.record_index.checked_add(2)?
        || bytes.get(position + 21..position + 27)? != [0; 6]
        || bytes.get(position + 27) != Some(&1)
        || !matches!(bytes.get(position + 28), Some(0 | 1))
        || bytes.get(position + 29) != Some(&0)
        || bytes.get(position + 30) != Some(&1)
        || View::u32_le_at(bytes, position + 31)? != header.record_index.checked_add(1)?
        || bytes.get(position + 35..position + 42)? != [0; 7]
        || bytes.get(position + 42) != Some(&1)
        || View::u32_le_at(bytes, position + 43)? != scope.record_index
        || bytes.get(position + 47..position + 53)? != [0; 6]
    {
        return None;
    }
    let paired_at = position.checked_add(53)?;
    let (paired_class_tag, after_paired_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_paired_tag)? != header.record_index {
        return None;
    }
    Some(DesignExtrudeSelectionGroup {
        id: String::new(),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        member_count_offset: u64::try_from(start + 32).ok()?,
        members,
        member_offsets,
        opaque_index,
        opaque_index_offset: u64::try_from(position).ok()?,
        opaque_scalar,
        opaque_scalar_offset: u64::try_from(position + 4).ok()?,
        variant: bytes[position + 28] != 0,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

/// Decode the fixed-width records named by Extrude selection groups.
pub fn decode_extrude_selection_members(
    scan: &ContainerScan,
    groups: &[DesignExtrudeSelectionGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignExtrudeSelectionMember>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut member) = parse_extrude_selection_member(bytes, group, ordinal, header)
            {
                member.id =
                    ids::native_design_extrude_selection_member_id(&entry.name, header.byte_offset);
                out.push(member);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Decode nested persistent-entity frames named by construction groups.
pub fn decode_entity_selection_operands(
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignEntitySelectionOperand>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            if let Some(mut operand) = parse_entity_selection_operand(bytes, group, ordinal, header)
            {
                operand.id =
                    ids::native_design_entity_selection_operand_id(&entry.name, header.byte_offset);
                out.push(operand);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(crate) fn parse_entity_selection_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignEntitySelectionOperand> {
    let frame = parse_entity_selection_frame(
        bytes,
        header.record_index,
        header.byte_offset,
        &header.class_tag,
    )?;
    Some(DesignEntitySelectionOperand {
        id: String::new(),
        scope_record_index: group.scope_record_index,
        group_record_index: group.record_index,
        group_member_ordinal,
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
        secondary_identity: frame.secondary_identity,
        secondary_identity_offset: frame.secondary_identity_offset,
        curve_secondary_identity: frame.curve_secondary_identity,
        curve_secondary_identity_offset: frame.curve_secondary_identity_offset,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: frame.next_record_index,
        next_byte_offset: frame.next_byte_offset,
    })
}

/// Persistent identity payload shared by entity-selection consumers that do
/// not belong to a construction-operand group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntitySelectionFrame {
    pub(crate) record_index: u32,
    pub(crate) byte_offset: u64,
    pub(crate) class_tag: String,
    pub(crate) asset_id: String,
    pub(crate) asset_id_offset: u64,
    pub(crate) context_id: String,
    pub(crate) context_id_offset: u64,
    pub(crate) identity_record_index: u32,
    pub(crate) identity_record_offset: u64,
    pub(crate) primary_identity: u64,
    pub(crate) primary_identity_offset: u64,
    pub(crate) secondary_identity: Option<u64>,
    pub(crate) secondary_identity_offset: Option<u64>,
    pub(crate) curve_secondary_identity: Option<u64>,
    pub(crate) curve_secondary_identity_offset: Option<u64>,
    pub(crate) next_record_index: u32,
    pub(crate) next_byte_offset: u64,
}

pub(crate) struct EntitySelectionPrefix {
    pub(crate) asset_id: String,
    pub(crate) asset_id_offset: u64,
    pub(crate) context_id: String,
    pub(crate) context_id_offset: u64,
    pub(crate) after_context_id: usize,
}

pub(crate) fn parse_entity_selection_prefix(
    bytes: &[u8],
    start: usize,
    record_index: u32,
) -> Option<EntitySelectionPrefix> {
    // Persistent selections use a ten-byte prelude and a u32 presence value.
    // Face-recipe selections add two prelude bytes, encode presence as one
    // byte, and add three zero bytes before the first UTF-16 length.
    let asset_start = if bytes.get(start + 11..start + coil_persist_sel::NESTED_SELECTION_MARKER)?
        == [0; 10]
        && bytes.get(start + coil_persist_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_persist_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(start + 26..start + coil_persist_sel::ASSET_PRESENCE)? == [0; 6]
        && View::u32_le_at(bytes, start + coil_persist_sel::ASSET_PRESENCE)? == 1
    {
        start.checked_add(coil_persist_sel::ASSET_UUID_LENGTH)?
    } else if bytes.get(start + 11..start + coil_modern_sel::NESTED_SELECTION_MARKER)? == [0; 11]
        && bytes.get(start + coil_modern_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_modern_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(
            start + coil_modern_sel::NESTED_RECORD_INDEX + 4
                ..start + coil_modern_sel::ASSET_PRESENCE,
        )? == [0; 6]
        && View::u32_le_at(bytes, start + coil_modern_sel::ASSET_PRESENCE)? == 1
    {
        start.checked_add(coil_modern_sel::ASSET_UUID_LENGTH)?
    } else if bytes.get(start + 11..start + coil_face_sel::NESTED_SELECTION_MARKER)? == [0; 12]
        && bytes.get(start + coil_face_sel::NESTED_SELECTION_MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + coil_face_sel::NESTED_RECORD_INDEX)?
            == record_index.checked_add(3)?
        && bytes.get(start + 28..start + coil_face_sel::ASSET_PRESENCE)? == [0; 6]
        && bytes.get(start + coil_face_sel::ASSET_PRESENCE) == Some(&1)
    {
        start.checked_add(coil_face_sel::ASSET_UUID_LENGTH)?
    } else {
        return None;
    };
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_start, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
        || bytes.get(after_context_id + 4..after_context_id + 8)? != [0; 4]
    {
        return None;
    }
    Some(EntitySelectionPrefix {
        asset_id,
        asset_id_offset: u64::try_from(asset_start.checked_add(4)?).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id.checked_add(4)?).ok()?,
        after_context_id,
    })
}

/// Match the selected curve identity carried by a nested entity-selection frame.
pub(crate) fn entity_selection_matches_curve(
    operand: &DesignEntitySelectionOperand,
    curve: &SketchCurveIdentity,
) -> bool {
    Some(curve.primary_id) == operand.secondary_identity
        && operand
            .curve_secondary_identity
            .is_none_or(|secondary| curve.secondary_id == secondary)
}

/// Direct sketch-point identity carried by a `WorkPoint` input.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkPointSketchPointFrame {
    class_tag: String,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    identity_record_index: u32,
    identity_record_offset: u64,
    sketch_record_index: u32,
    sketch_record_index_offset: u64,
    point_persistent_id: u64,
    point_persistent_id_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

/// Parse the direct sketch-point identity variant of a `WorkPoint` input.
///
/// The outer prefix is shared with persistent entity selections. Its identity
/// record is a separate four-record envelope: nine zero bytes, a one-byte
/// presence marker, two marked `u32` slots separated by zero `u32` values,
/// and the following point-data record.
fn parse_work_point_sketch_point_frame(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
    class_tag: &str,
) -> Option<WorkPointSketchPointFrame> {
    let start = usize::try_from(byte_offset).ok()?;
    let prefix = parse_entity_selection_prefix(bytes, start, record_index)?;
    let paired_at = next_indexed_record_offset(bytes, prefix.after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + indexed_header::LEN)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + indexed_header::LEN)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + indexed_header::LEN)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + indexed_header::LEN)?;
    let expected = [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (_, after_next_tag) = lp_ascii_filtered(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = View::u32_le_at(bytes, after_next_tag)?;
    if next_record_index != record_index.checked_add(4)?
        || bytes
            .get(identity_at + indexed_header::LEN..identity_at + sketch_point_identity::PRESENCE)?
            != [0; 9]
        || bytes.get(identity_at + sketch_point_identity::PRESENCE) != Some(&1)
        || View::u32_le_at(bytes, identity_at + sketch_point_identity::PRESENCE + 1)? != 0
        || View::u32_le_at(
            bytes,
            identity_at + sketch_point_identity::SKETCH_RECORD_INDEX + 4,
        )? != 0
        || identity_at.checked_add(sketch_point_identity::LEN)? != next_at
    {
        return None;
    }
    let sketch_record_index = View::u32_le_at(
        bytes,
        identity_at + sketch_point_identity::SKETCH_RECORD_INDEX,
    )?;
    let point_persistent_id = u64::from(View::u32_le_at(
        bytes,
        identity_at + sketch_point_identity::POINT_PERSISTENT_ID,
    )?);
    Some(WorkPointSketchPointFrame {
        class_tag: class_tag.to_owned(),
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        identity_record_index: record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        sketch_record_index,
        sketch_record_index_offset: u64::try_from(
            identity_at.checked_add(sketch_point_identity::SKETCH_RECORD_INDEX)?,
        )
        .ok()?,
        point_persistent_id,
        point_persistent_id_offset: u64::try_from(
            identity_at.checked_add(sketch_point_identity::POINT_PERSISTENT_ID)?,
        )
        .ok()?,
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Parse the nested persistent-entity frame without assigning group ownership.
pub(crate) fn parse_entity_selection_frame(
    bytes: &[u8],
    record_index: u32,
    byte_offset: u64,
    class_tag: &str,
) -> Option<EntitySelectionFrame> {
    let start = usize::try_from(byte_offset).ok()?;
    let prefix = parse_entity_selection_prefix(bytes, start, record_index)?;
    let paired_at = next_indexed_record_offset(bytes, prefix.after_context_id + 8)?;
    let nested_one_at = next_indexed_record_offset(bytes, paired_at + 11)?;
    let nested_two_at = next_indexed_record_offset(bytes, nested_one_at + 11)?;
    let identity_at = next_indexed_record_offset(bytes, nested_two_at + 11)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 11)?;
    let expected = [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ];
    for (offset, expected) in [paired_at, nested_one_at, nested_two_at, identity_at]
        .into_iter()
        .zip(expected)
    {
        let (_, after_tag) = lp_ascii_filtered(bytes, offset, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_tag)? != expected {
            return None;
        }
    }
    let (identity_class_tag, _) =
        lp_ascii_filtered(bytes, identity_at, 0..=2000, u8::is_ascii_graphic)?;
    let (_, after_next_tag) = lp_ascii_filtered(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
    let next_record_index = View::u32_le_at(bytes, after_next_tag)?;
    let (
        primary_identity_offset,
        secondary_identity_offset,
        curve_secondary_identity_offset,
        primary_identity,
        secondary_identity,
        curve_secondary_identity,
    ) = if class_tag == "338"
        && identity_class_tag == "361"
        && bytes.get(
            identity_at + class_338_curve::ZERO_PREFIX..identity_at + class_338_curve::PRESENCE,
        )? == [0; 9]
        && bytes.get(identity_at + class_338_curve::PRESENCE) == Some(&1)
        && bytes.get(
            identity_at + class_338_curve::PRESENCE + 1
                ..identity_at + class_338_curve::OWNER_RECORD_INDEX,
        )? == [0; 12]
        && View::u32_le_at(bytes, identity_at + class_338_curve::OWNER_HIGH_ZERO)? == 0
        && View::u32_le_at(bytes, identity_at + class_338_curve::CURVE_HIGH_ZERO)? == 0
        && identity_at.checked_add(class_338_curve::LEN)? == next_at
        && next_record_index == record_index.checked_add(4)?
    {
        let primary_identity = u64::from(View::u32_le_at(
            bytes,
            identity_at + class_338_curve::OWNER_RECORD_INDEX,
        )?);
        let secondary_identity = u64::from(View::u32_le_at(
            bytes,
            identity_at + class_338_curve::CURVE_PERSISTENT_ID,
        )?);
        (
            identity_at.checked_add(class_338_curve::OWNER_RECORD_INDEX)?,
            Some(identity_at.checked_add(class_338_curve::CURVE_PERSISTENT_ID)?),
            None,
            primary_identity,
            Some(secondary_identity),
            None,
        )
    } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
        && identity_at.checked_add(45)? == next_at
        && next_record_index == record_index.checked_add(4)?
    {
        let curve_secondary_identity_offset = identity_at.checked_add(21)?;
        let primary_identity_offset = identity_at.checked_add(29)?;
        let secondary_identity_offset = identity_at.checked_add(37)?;
        (
            primary_identity_offset,
            Some(secondary_identity_offset),
            Some(curve_secondary_identity_offset),
            View::u64_le_at(bytes, primary_identity_offset)?,
            Some(View::u64_le_at(bytes, secondary_identity_offset)?),
            Some(View::u64_le_at(bytes, curve_secondary_identity_offset)?),
        )
    } else if bytes.get(identity_at + 11..identity_at + 21)? == [0; 10]
        && identity_at.checked_add(29)? == next_at
    {
        let primary_identity_offset = identity_at.checked_add(21)?;
        (
            primary_identity_offset,
            None,
            None,
            View::u64_le_at(bytes, primary_identity_offset)?,
            None,
            None,
        )
    } else {
        return None;
    };
    Some(EntitySelectionFrame {
        record_index,
        byte_offset,
        class_tag: class_tag.to_owned(),
        asset_id: prefix.asset_id,
        asset_id_offset: prefix.asset_id_offset,
        context_id: prefix.context_id,
        context_id_offset: prefix.context_id_offset,
        identity_record_index: record_index.checked_add(3)?,
        identity_record_offset: u64::try_from(identity_at).ok()?,
        primary_identity,
        primary_identity_offset: u64::try_from(primary_identity_offset).ok()?,
        secondary_identity,
        secondary_identity_offset: secondary_identity_offset
            .and_then(|offset| u64::try_from(offset).ok()),
        curve_secondary_identity: curve_secondary_identity.filter(|identity| *identity != 0),
        curve_secondary_identity_offset: curve_secondary_identity_offset
            .filter(|offset| View::u64_le_at(bytes, *offset).is_some_and(|identity| identity != 0))
            .and_then(|offset| u64::try_from(offset).ok()),
        next_record_index,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Decode whole-body construction operands that contain one persistent body recipe.
pub fn decode_body_recipe_operands(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignBodyRecipeOperand>, CodecError> {
    let mut headers_by_identity = HashMap::<_, Option<&DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        headers_by_identity
            .entry((stream, header.record_index))
            .and_modify(|header| *header = None)
            .or_insert(Some(header));
    }
    let mut body_recipes_by_stream = HashMap::<_, Vec<&ConstructionRecipe>>::new();
    for recipe in recipes
        .iter()
        .filter(|recipe| recipe.kind == ConstructionRecipeKind::Body)
    {
        let Some(stream) = native_stream(&recipe.id) else {
            continue;
        };
        body_recipes_by_stream
            .entry(stream)
            .or_default()
            .push(recipe);
    }
    for stream_recipes in body_recipes_by_stream.values_mut() {
        stream_recipes.sort_by_key(|recipe| recipe.byte_offset);
    }
    let mut record_offset_index: HashMap<&str, IndexedRecordOffsets> = HashMap::new();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        if scopes.iter().any(|scope| {
            scope.record_index == group.scope_record_index
                && native_stream(&scope.id) == Some(stream)
                && scope.kind == "Hole"
        }) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        for (ordinal, record_index) in group.members.iter().copied().enumerate() {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(Some(header)) = headers_by_identity.get(&(stream, record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe_with_index(
                records,
                header,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            if let Some(mut operand) =
                parse_body_recipe_operand_with_index(bytes, records, group, ordinal, header, recipe)
            {
                operand.id =
                    ids::native_design_body_recipe_operand_id(&entry.name, header.byte_offset);
                out.push(operand);
            }
        }
    }
    for scope in scopes
        .iter()
        .filter(|scope| scope.combine_operation().is_some() || scope.kind == "Hole")
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(role::BULKSTREAM, stream) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = record_offset_index
            .entry(stream)
            .or_insert_with(|| IndexedRecordOffsets::build(bytes));
        let combine_record_indexes = scope.combine_operation().map(|operation| {
            std::iter::once(operation.target.record_index)
                .chain(operation.tools.iter().map(|tool| tool.record_index))
                .collect::<Vec<_>>()
        });
        let record_indexes = combine_record_indexes
            .as_deref()
            .unwrap_or(&scope.reference_members);
        for record_index in record_indexes {
            let mut ordinals = scope
                .reference_members
                .iter()
                .enumerate()
                .filter(|(_, member)| *member == record_index)
                .filter_map(|(ordinal, _)| u32::try_from(ordinal).ok());
            let Some(scope_reference_ordinal) = ordinals.next() else {
                continue;
            };
            if ordinals.next().is_some()
                || scope
                    .combine_operation()
                    .is_some_and(|_| scope_reference_ordinal.is_multiple_of(2))
            {
                continue;
            }
            let Some(Some(header)) = headers_by_identity.get(&(stream, *record_index)) else {
                continue;
            };
            let Some(recipe) = unique_body_recipe_with_index(
                records,
                header,
                body_recipes_by_stream
                    .get(stream)
                    .map_or(&[], Vec::as_slice),
            ) else {
                continue;
            };
            let owner = DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            };
            if let Some(mut operand) = parse_body_recipe_operand_frame_with_index(
                bytes,
                records,
                scope.record_index,
                owner,
                header,
                recipe,
            ) {
                operand.id =
                    ids::native_design_body_recipe_operand_id(&entry.name, header.byte_offset);
                out.push(operand);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    let mut owner_counts = HashMap::new();
    for operand in &out {
        *owner_counts.entry(operand.id.clone()).or_insert(0_u32) += 1;
    }
    out.retain(|operand| owner_counts.get(&operand.id) == Some(&1));
    Ok(out)
}

/// Select the sole body recipe in the structural interval after the `N+3`
/// header and before the enclosing `N+4` header. The bounded Design stream,
/// rather than an arbitrary byte distance, limits the interval.
#[cfg(test)]
fn unique_body_recipe<'a>(
    bytes: &[u8],
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let records = IndexedRecordOffsets::build(bytes);
    unique_body_recipe_with_index(&records, header, recipes)
}

fn unique_body_recipe_with_index<'a>(
    records: &IndexedRecordOffsets,
    header: &DesignRecordHeader,
    recipes: &'a [&'a ConstructionRecipe],
) -> Option<&'a ConstructionRecipe> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let prologue_end = body_recipe_prologue_end_with_index(records, start, header.record_index)?;
    let next_at = records.first_at_or_after(prologue_end, header.record_index.checked_add(4)?)?;
    let lower = u64::try_from(prologue_end).ok()?;
    let upper = u64::try_from(next_at).ok()?;
    let matching = &recipes[recipes.partition_point(|recipe| recipe.byte_offset < lower)
        ..recipes.partition_point(|recipe| recipe.byte_offset < upper)];
    let [recipe] = matching else {
        return None;
    };
    Some(recipe)
}

/// Offset past the four consecutively indexed records that open a body-recipe
/// operand, or `None` when the records after `start` do not carry
/// `record_index` through `record_index + 3` in order.
///
/// The prologue depends only on the operand header, so a caller weighing many
/// candidate recipes against one header resolves it once.
#[cfg(test)]
fn body_recipe_prologue_end(bytes: &[u8], start: usize, record_index: u32) -> Option<usize> {
    let records = IndexedRecordOffsets::build(bytes);
    body_recipe_prologue_end_with_index(&records, start, record_index)
}

fn body_recipe_prologue_end_with_index(
    records: &IndexedRecordOffsets,
    start: usize,
    record_index: u32,
) -> Option<usize> {
    let mut search = start.checked_add(11)?;
    for expected in [
        record_index,
        record_index.checked_add(1)?,
        record_index.checked_add(2)?,
        record_index.checked_add(3)?,
    ] {
        let at = records.first_at_or_after(search, expected)?;
        search = at.checked_add(11)?;
    }
    Some(search)
}

/// Offset of the record carrying `record_index + 4` that closes a body-recipe
/// operand whose recipe sits at `recipe_at`. The recipe must follow the
/// prologue that ends at `prologue_end`.
#[cfg(test)]
fn body_recipe_operand_end(
    bytes: &[u8],
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<usize> {
    let records = IndexedRecordOffsets::build(bytes);
    body_recipe_operand_end_with_index(&records, prologue_end, record_index, recipe_at)
}

fn body_recipe_operand_end_with_index(
    records: &IndexedRecordOffsets,
    prologue_end: usize,
    record_index: u32,
    recipe_at: usize,
) -> Option<usize> {
    if recipe_at < prologue_end {
        return None;
    }
    records.first_at_or_after(recipe_at, record_index.checked_add(4)?)
}

#[cfg(test)]
pub(crate) fn parse_body_recipe_operand(
    bytes: &[u8],
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    let records = IndexedRecordOffsets::build(bytes);
    parse_body_recipe_operand_with_index(
        bytes,
        &records,
        group,
        group_member_ordinal,
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    parse_body_recipe_operand_frame_with_index(
        bytes,
        records,
        group.scope_record_index,
        DesignOperandOwner::Group {
            group_record_index: group.record_index,
            group_member_ordinal,
        },
        header,
        recipe,
    )
}

fn parse_body_recipe_operand_frame_with_index(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope_record_index: u32,
    owner: DesignOperandOwner,
    header: &DesignRecordHeader,
    recipe: &ConstructionRecipe,
) -> Option<DesignBodyRecipeOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let recipe_at = usize::try_from(recipe.byte_offset).ok()?;
    let prologue_end = body_recipe_prologue_end_with_index(records, start, header.record_index)?;
    let next_at =
        body_recipe_operand_end_with_index(records, prologue_end, header.record_index, recipe_at)?;
    let reference_count = usize::try_from(View::u32_le_at(bytes, start + 21)?).ok()?;
    // The legacy Combine form permits an empty persistent-reference table;
    // its marker then starts at the ordinary post-count cursor. The history
    // binder keeps that operand native because an empty identity cannot prove
    // a body selection.
    if start >= recipe_at || recipe_at >= next_at || bytes.get(start + 11..start + 21)? != [0; 10] {
        return None;
    }
    let mut cursor = start.checked_add(25)?;
    // Each reference consumes 12 bytes; a count the remaining bytes cannot
    // supply is corrupt and must not reach the allocator.
    if reference_count > bytes.len().saturating_sub(cursor) / 12 {
        return None;
    }
    let mut references = Vec::with_capacity(reference_count);
    for _ in 0..reference_count {
        references.push(DesignBodyRecipeReference {
            design_reference: View::u64_le_at(bytes, cursor)?,
            design_reference_offset: u64::try_from(cursor).ok()?,
            form: View::u32_le_at(bytes, cursor + 8)?,
            form_offset: u64::try_from(cursor + 8).ok()?,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        });
        cursor = cursor.checked_add(12)?;
    }
    if bytes.get(cursor) != Some(&1)
        || bytes.get(cursor + 9..cursor + 11)? != [0; 2]
        || View::u32_le_at(bytes, cursor + 11)? != 1
    {
        return None;
    }
    let nested_record_index = View::u64_le_at(bytes, cursor + 1)?;
    let asset_id_at = cursor.checked_add(15)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, asset_id_at, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let selector_tail_at = after_context_id.checked_add(4)?;
    let selector_tail: [u8; 4] = bytes
        .get(selector_tail_at..selector_tail_at.checked_add(4)?)?
        .try_into()
        .ok()?;
    let selector_tail_is_valid = match header.class_tag.as_str() {
        // The class-365 member is a four-byte generation-dependent value.
        // Retain it until DR-24 identifies its neutral meaning.
        "365" => true,
        "367" => selector_tail == [1, 0, 0, 0],
        _ => selector_tail == [0; 4],
    };
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
        || !selector_tail_is_valid
        || nested_record_index != u64::from(header.record_index.checked_add(3)?)
    {
        return None;
    }
    Some(DesignBodyRecipeOperand {
        id: String::new(),
        scope_record_index,
        owner,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        asset_id,
        asset_id_offset: u64::try_from(asset_id_at + 4).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        selector_tail: Some(selector_tail),
        selector_tail_offset: Some(u64::try_from(selector_tail_at).ok()?),
        references,
        nested_record_index,
        nested_record_index_offset: u64::try_from(cursor + 1).ok()?,
        recipe_id: recipe.id.clone(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: header.record_index.checked_add(4)?,
        next_byte_offset: u64::try_from(next_at).ok()?,
    })
}

/// Join body-recipe Design references to solved persistent face tags.
pub fn bind_body_recipe_operand_candidates(
    operands: &mut [DesignBodyRecipeOperand],
    recipes: &[ConstructionRecipe],
    tags: &[PersistentSubentityTag],
    scopes: &[DesignParameterScope],
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut recipes_by_id = HashMap::<_, Option<&ConstructionRecipe>>::new();
    for recipe in recipes {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    for operand in operands {
        // The recipe selector is a persistent-tag selector for Combine's
        // form-three clauses. In the class-365 body-member grammar it names
        // the enclosing N+4 record instead, so each clause joins by its own
        // Design reference and the history pass performs the body proof.
        let form_three_uses_recipe_selector = {
            let mut matching_scopes = scopes.iter().filter(|scope| {
                scope.record_index == operand.scope_record_index
                    && native_stream(&scope.id) == native_stream(&operand.id)
            });
            match (matching_scopes.next(), matching_scopes.next()) {
                (Some(scope), None) => scope.combine_operation().is_some(),
                _ => true,
            }
        };
        let tag_selector = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe)
            .and_then(|recipe| recipe.design_selector)
            .map(|selector| i64::from(selector.value));
        for reference in &mut operand.references {
            reference.candidate_faces.clear();
            let Ok(design_reference) = i64::try_from(reference.design_reference) else {
                continue;
            };
            reference.candidate_faces = tags
                .iter()
                .filter(|tag| {
                    crate::ids::same_native_occurrence(&tag.id, &operand.id)
                        && tag.design_references.contains(&design_reference)
                        && (reference.form != 3
                            || !form_three_uses_recipe_selector
                            || tag_selector == Some(tag.selector))
                })
                .filter_map(|tag| match &tag.target {
                    AttributeTarget::Face(face) => Some(face.clone()),
                    _ => None,
                })
                .collect();
            reference
                .candidate_faces
                .sort_by(|left, right| left.0.cmp(&right.0));
            reference.candidate_faces.dedup();
        }
    }
}

/// Resolve selection-member local identities against persistent point and
/// curve identities owned by the Extrude scope's selected Sketch.
pub fn bind_extrude_selection_geometry(
    members: &mut [DesignExtrudeSelectionMember],
    groups: &[DesignExtrudeSelectionGroup],
    scopes: &[DesignParameterScope],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) {
    let selected_sketches = groups
        .iter()
        .filter_map(|group| {
            let stream = native_stream(&group.id)?;
            let scope = scopes.iter().find(|scope| {
                native_stream(&scope.id) == Some(stream)
                    && scope.record_index == group.scope_record_index
            })?;
            Some((
                (stream, group.record_index),
                scope.extrude_profile()?.entity_suffix,
            ))
        })
        .collect::<HashMap<_, _>>();
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let Some(entity_suffix) = selected_sketches.get(&(stream, member.group_record_index))
        else {
            continue;
        };
        let Ok(entity_suffix) = u32::try_from(*entity_suffix) else {
            continue;
        };
        let point_operands = points.iter().filter_map(|point| {
            (native_stream(&point.id) == Some(stream)
                && point.owner_reference == Some(entity_suffix)
                && point.persistent_id() == Some(member.local_id))
            .then_some(SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_operands = curves.iter().filter_map(|curve| {
            (native_stream(&curve.id) == Some(stream)
                && curve.owner_reference == Some(entity_suffix)
                && (curve.primary_id == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                })
        });
        let matches = point_operands.chain(curve_operands).collect::<Vec<_>>();
        if let [resolved] = matches.as_slice() {
            member.resolved_geometry = Some(resolved.clone());
        }
    }
}

/// Bind selection members to construction-operand identity chains that
/// terminate at the same fixed persistent identity record.
pub fn bind_extrude_selection_identities(
    members: &mut [DesignExtrudeSelectionMember],
    identities: &[DesignConstructionOperandIdentity],
) {
    for member in members {
        let Some(stream) = native_stream(&member.id) else {
            continue;
        };
        let mut matches = identities
            .iter()
            .filter(|identity| {
                native_stream(&identity.id) == Some(stream)
                    && identity.following_record_index == member.record_index
                    && identity.following_byte_offset == member.byte_offset
                    && identity
                        .persistent_identity
                        .as_ref()
                        .is_some_and(|persistent| {
                            persistent.local_id == member.local_id
                                && persistent.asset_id == member.asset_id
                                && persistent.context_id == member.context_id
                        })
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|identity| identity.wrapper_byte_offsets.first().copied());
        member.operand_identity_ids = matches
            .into_iter()
            .map(|identity| identity.id.clone())
            .collect();
    }
}

pub(crate) fn parse_extrude_selection_member(
    bytes: &[u8],
    group: &DesignExtrudeSelectionGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<DesignExtrudeSelectionMember> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let member = parse_extrude_identity_member(bytes, start)?;
    Some(DesignExtrudeSelectionMember {
        id: String::new(),
        group_record_index: group.record_index,
        group_member_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        local_id: member.local_id,
        local_id_offset: member.local_id_offset,
        asset_id: member.asset_id,
        asset_id_offset: member.asset_id_offset,
        context_id: member.context_id,
        context_id_offset: member.context_id_offset,
        tail_slot_present: member.tail_slot_present,
        tail_slot_offset: member.tail_slot_offset,
        resolved_geometry: None,
        operand_identity_ids: Vec::new(),
        historical: None,
        next_record_index: member.next_record_index,
        next_byte_offset: member.next_byte_offset,
    })
}

struct ParsedExtrudeIdentityMember {
    local_id: u64,
    local_id_offset: u64,
    asset_id: String,
    asset_id_offset: u64,
    context_id: String,
    context_id_offset: u64,
    tail_slot_present: bool,
    tail_slot_offset: u64,
    next_record_index: u32,
    next_byte_offset: u64,
}

fn parse_extrude_identity_member(
    bytes: &[u8],
    start: usize,
) -> Option<ParsedExtrudeIdentityMember> {
    if bytes.get(start + extrude_member::ZERO_RUN_10..start + extrude_member::LOCAL_IDENTITY)?
        != [0; 10]
    {
        return None;
    }
    let local_id = View::u64_le_at(bytes, start + extrude_member::LOCAL_IDENTITY)?;
    let (asset_id, after_asset_id) =
        lp_utf16_bounded(bytes, start + extrude_member::ASSET_UUID_LENGTH, 1..=256)?;
    let (context_id, after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let tail_slot_offset = after_context_id.checked_add(4)?;
    let tail_slot_present = match bytes.get(tail_slot_offset)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if !is_guid_relaxed(&asset_id)
        || !is_guid_relaxed(&context_id)
        || View::u32_le_at(bytes, after_context_id)? != 2
    {
        return None;
    }
    let fixed_end = start.checked_add(extrude_member::LEN)?;
    let (next_record_index, next_byte_offset) = if View::u32_le_at(bytes, tail_slot_offset + 1)
        == Some(0)
        && after_context_id.checked_add(9)? == fixed_end
    {
        if fixed_end == bytes.len() {
            (0, u64::try_from(fixed_end).ok()?)
        } else {
            let (_, after_next_tag) =
                lp_ascii_filtered(bytes, fixed_end, 0..=2000, u8::is_ascii_graphic)?;
            (
                View::u32_le_at(bytes, after_next_tag)?,
                u64::try_from(fixed_end).ok()?,
            )
        }
    } else if bytes.get(tail_slot_offset + 1..tail_slot_offset + 4)? == [0; 3] {
        let mut cursor = tail_slot_offset.checked_add(4)?;
        let (next_record_index, _) = take_record_reference(bytes, &mut cursor)?;
        let next_at = cursor;
        let (_, after_next_tag) =
            lp_ascii_filtered(bytes, next_at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_next_tag)? != next_record_index {
            return None;
        }
        (next_record_index, u64::try_from(next_at).ok()?)
    } else {
        return None;
    };
    Some(ParsedExtrudeIdentityMember {
        local_id,
        local_id_offset: u64::try_from(start + extrude_member::LOCAL_IDENTITY).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + extrude_member::ASSET_UUID_UTF16).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
        tail_slot_present,
        tail_slot_offset: u64::try_from(tail_slot_offset).ok()?,
        next_record_index,
        next_byte_offset,
    })
}

pub(crate) struct ParsedEdgeIdentityMember {
    pub(crate) compact_layout: bool,
    pub(crate) local_id: u64,
    pub(crate) local_id_offset: u64,
    pub(crate) asset_id: String,
    pub(crate) asset_id_offset: u64,
    pub(crate) context_id: String,
    pub(crate) context_id_offset: u64,
}

pub(crate) fn parse_edge_identity_member(
    bytes: &[u8],
    start: usize,
) -> Option<ParsedEdgeIdentityMember> {
    let (compact_layout, marker_offset) = if bytes.get(start + 11..start + 23) == Some(&[0; 12]) {
        (false, 23)
    } else if bytes.get(start + 11..start + 22) == Some(&[0; 11]) {
        (true, 22)
    } else if bytes.get(start + 11..start + 21) == Some(&[0; 10]) {
        (true, 21)
    } else {
        return None;
    };
    let local_id_offset = marker_offset + 1;
    let asset_offset = marker_offset + 15;
    if bytes.get(start + marker_offset) != Some(&1)
        || bytes.get(start + marker_offset + 5..start + marker_offset + 11)? != [0; 6]
        || View::u32_le_at(bytes, start + marker_offset + 11)? != 1
    {
        return None;
    }
    let local_id = u64::from(View::u32_le_at(bytes, start + local_id_offset)?);
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + asset_offset, 1..=256)?;
    let (context_id, _after_context_id) = lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    if !is_guid_relaxed(&asset_id) || !is_guid_relaxed(&context_id) {
        return None;
    }
    Some(ParsedEdgeIdentityMember {
        compact_layout,
        local_id,
        local_id_offset: u64::try_from(start + local_id_offset).ok()?,
        asset_id,
        asset_id_offset: u64::try_from(start + asset_offset + 4).ok()?,
        context_id,
        context_id_offset: u64::try_from(after_asset_id + 4).ok()?,
    })
}

pub(crate) fn parse_sketch_profile(
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
) -> Option<DesignSketchProfileOperand> {
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
    if !is_guid_relaxed(&asset_id) {
        return None;
    }
    let (entity_suffix_text, after_entity_suffix) =
        lp_utf16_bounded(bytes, after_asset_id, 1..=256)?;
    let entity_suffix = entity_suffix_text.parse::<u64>().ok()?;
    let paired_at = next_indexed_record_offset(bytes, start + 11)?;
    let (paired_class_tag, after_paired_tag) =
        lp_ascii_filtered(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let tail_length = paired_at.checked_sub(after_entity_suffix)?;
    if View::u32_le_at(bytes, after_paired_tag)? != header.record_index
        || !matches!(tail_length, 89 | 93 | 94)
    {
        return None;
    }
    if matches!(tail_length, 89 | 93) {
        let tail = after_entity_suffix;
        let (nested_two_at, nested_one_at, scope_at) = if tail_length == 89 {
            (53, 66, 78)
        } else {
            (57, 70, 82)
        };
        if bytes.get(tail..tail + 8) != Some(&[1, 0, 0, 0, 0, 0, 0, 0])
            || View::u32_le_at(bytes, tail + 8) != Some(1)
            || marked_record_reference(bytes, tail + nested_two_at)
                != header.record_index.checked_add(2)
            || bytes.get(tail + nested_one_at - 2..tail + nested_one_at) != Some(&[0; 2])
            || marked_record_reference(bytes, tail + nested_one_at)
                != header.record_index.checked_add(1)
            || bytes.get(tail + scope_at - 1) != Some(&0)
            || marked_record_reference(bytes, tail + scope_at).is_none()
            || View::u32_le_at(bytes, tail + 41) == Some(0)
            || (tail_length == 93
                && View::u32_le_at(bytes, tail + 41) != View::u32_le_at(bytes, tail + 53))
        {
            return None;
        }
    }
    let matches = entities
        .iter()
        .filter(|entity| {
            native_stream(&entity.id) == Some(stream)
                && entity.in_sketch_module()
                && entity.entity_suffix == entity_suffix
        })
        .collect::<Vec<_>>();
    let [entity] = matches.as_slice() else {
        return None;
    };
    let region_selection =
        parse_sketch_profile_region_selection(bytes, header.record_index, paired_at);
    Some(DesignSketchProfileOperand {
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        asset_id,
        asset_id_offset: u64::try_from(start + 40).ok()?,
        entity_id: entity.entity_id.clone(),
        entity_suffix,
        entity_reference_offset: u64::try_from(after_asset_id + 4).ok()?,
        region_selection,
        paired_class_tag,
        paired_byte_offset: u64::try_from(paired_at).ok()?,
    })
}

fn parse_sketch_profile_region_selection(
    bytes: &[u8],
    profile_record_index: u32,
    paired_at: usize,
) -> Option<DesignSketchProfileRegionSelection> {
    const REGION_MARKER_LEN: usize = 1;
    const REGION_COUNT_LEN: usize = 4;
    const TERMINATOR_LEN: usize = 5;

    let next_header = |position, expected| {
        let at = next_indexed_record_offset(bytes, position)?;
        (indexed_record_index(bytes, at) == Some(expected)).then_some(at)
    };
    let nested_one_at = next_header(
        paired_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(1)?,
    )?;
    let nested_two_at = next_header(
        nested_one_at.checked_add(indexed_header::LEN)?,
        profile_record_index.checked_add(2)?,
    )?;
    let selection_record_index = profile_record_index.checked_add(3)?;
    let selection_at = next_header(
        nested_two_at.checked_add(indexed_header::LEN)?,
        selection_record_index,
    )?;
    let (class_tag, after_class_tag) =
        lp_ascii_filtered(bytes, selection_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_class_tag)? != selection_record_index
        || bytes.get(
            selection_at + region_selection::ZERO_RUN_10
                ..selection_at + region_selection::PROFILE_REFERENCE_MARKER,
        )? != [0; 10]
        || marked_record_reference(
            bytes,
            selection_at + region_selection::PROFILE_REFERENCE_MARKER,
        ) != Some(profile_record_index)
        || bytes.get(
            selection_at + region_selection::ZERO_RUN_6
                ..selection_at + region_selection::FORMAT_VERSION,
        )? != [0; 6]
        || View::u32_le_at(bytes, selection_at + region_selection::FORMAT_VERSION)? != 1
    {
        return None;
    }
    let region_count = usize::try_from(View::u32_le_at(
        bytes,
        selection_at + region_selection::REGION_COUNT,
    )?)
    .ok()?;
    if region_count == 0 {
        return None;
    }
    let minimum_regions_len = region_count
        .checked_mul(REGION_COUNT_LEN.checked_add(region_member::LEN)?)?
        .checked_add(
            region_count
                .checked_sub(1)?
                .checked_mul(REGION_MARKER_LEN)?,
        )?
        .checked_add(TERMINATOR_LEN)?
        .checked_add(indexed_header::LEN)?;
    let mut cursor = selection_at.checked_add(region_selection::LEN)?;
    if cursor.checked_add(minimum_regions_len)? > bytes.len() {
        return None;
    }
    let mut regions = Vec::with_capacity(region_count.min(4096));
    for region_ordinal in 0..region_count {
        if region_ordinal != 0 {
            if bytes.get(cursor) != Some(&1) {
                return None;
            }
            cursor = cursor.checked_add(1)?;
        }
        let member_count_offset = cursor;
        let member_count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
        cursor = cursor.checked_add(REGION_COUNT_LEN)?;
        if member_count == 0 {
            return None;
        }
        let remaining_regions = region_count.checked_sub(region_ordinal.checked_add(1)?)?;
        let trailing_minimum_len = remaining_regions
            .checked_mul(
                REGION_MARKER_LEN
                    .checked_add(REGION_COUNT_LEN)?
                    .checked_add(region_member::LEN)?,
            )?
            .checked_add(TERMINATOR_LEN)?
            .checked_add(indexed_header::LEN)?;
        let member_run_len = member_count.checked_mul(region_member::LEN)?;
        if cursor
            .checked_add(member_run_len)?
            .checked_add(trailing_minimum_len)?
            > bytes.len()
        {
            return None;
        }
        let mut members = Vec::with_capacity(member_count.min(4096));
        for _ in 0..member_count {
            let kind_offset = cursor;
            let kind = View::u32_le_at(bytes, cursor)?;
            let curve_primary_id = u64::from(View::u32_le_at(
                bytes,
                cursor.checked_add(region_member::CURVE_PRIMARY_ID)?,
            )?);
            let incidence_words_offset = cursor.checked_add(region_member::ZERO_WORDS_3)?;
            let mut incidence_words = [0; 8];
            for (ordinal, word) in incidence_words.iter_mut().enumerate() {
                *word = View::u32_le_at(
                    bytes,
                    incidence_words_offset.checked_add(ordinal.checked_mul(4)?)?,
                )?;
            }
            if kind != 3
                || curve_primary_id == 0
                || incidence_words[..3] != [0; 3]
                || !matches!(incidence_words[3], 0 | 1)
                || !matches!(incidence_words[4], 1 | 2)
                || !matches!(incidence_words[5], 1 | 2)
                || incidence_words[6..] != [0; 2]
            {
                return None;
            }
            members.push(DesignSketchProfileRegionMember {
                kind,
                kind_offset: u64::try_from(kind_offset).ok()?,
                curve_primary_id,
                curve_primary_id_offset: u64::try_from(
                    cursor.checked_add(region_member::CURVE_PRIMARY_ID)?,
                )
                .ok()?,
                incidence_words,
                incidence_words_offset: u64::try_from(incidence_words_offset).ok()?,
            });
            cursor = cursor.checked_add(region_member::LEN)?;
        }
        regions.push(DesignSketchProfileRegion {
            member_count_offset: u64::try_from(member_count_offset).ok()?,
            members,
        });
    }
    let companion_at = cursor.checked_add(TERMINATOR_LEN)?;
    if bytes.get(cursor..companion_at)? != [0; TERMINATOR_LEN]
        || next_header(companion_at, selection_record_index)? != companion_at
    {
        return None;
    }
    let (companion_class_tag, after_companion_class_tag) =
        lp_ascii_filtered(bytes, companion_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_companion_class_tag)? != selection_record_index {
        return None;
    }
    Some(DesignSketchProfileRegionSelection {
        record_index: selection_record_index,
        byte_offset: u64::try_from(selection_at).ok()?,
        class_tag,
        region_count_offset: u64::try_from(
            selection_at.checked_add(region_selection::REGION_COUNT)?,
        )
        .ok()?,
        regions,
        companion_class_tag,
        companion_byte_offset: u64::try_from(companion_at).ok()?,
    })
}

fn marked_record_reference(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1) || bytes.get(at + 5..at + 11)? != [0; 6] {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}

struct ParsedRecipeOperand {
    paired_byte_offset: u64,
    paired_class_tag: String,
    recipe_record_index: u32,
    recipe_record_byte_offset: u64,
    recipe_id: String,
    recipe_prefix_offset: u64,
    recipe_prefix_bytes: Vec<u8>,
    recipe_references: Vec<crate::records::DesignRecipeReference>,
    recipe_program_offset: u64,
    recipe_program: Vec<i32>,
    next_record_index: u32,
    next_byte_offset: u64,
}

#[derive(Clone, Copy)]
enum RecipeOperandTerminator {
    RecordDelta(u32),
    NextIndexedAfterRecipe { limit: u64 },
}

/// Parse one exact persistent vertex-recipe envelope.
pub(crate) fn parse_vertex_recipe(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignVertexRecipe> {
    let parsed = parse_recipe_operand(
        bytes,
        records,
        stream,
        header,
        recipes,
        ConstructionRecipeKind::Vertex,
        RecipeOperandTerminator::RecordDelta(5),
    )?;
    Some(DesignVertexRecipe {
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: parsed.paired_byte_offset,
        paired_class_tag: parsed.paired_class_tag,
        recipe_record_index: parsed.recipe_record_index,
        recipe_record_byte_offset: parsed.recipe_record_byte_offset,
        recipe_id: parsed.recipe_id,
        recipe_prefix_offset: parsed.recipe_prefix_offset,
        recipe_prefix_bytes: parsed.recipe_prefix_bytes,
        recipe_references: parsed.recipe_references,
        recipe_program_offset: parsed.recipe_program_offset,
        recipe_program: parsed.recipe_program,
        recipe_state_id: None,
        resolved_vertex_slot: None,
        next_record_index: parsed.next_record_index,
        next_byte_offset: parsed.next_byte_offset,
    })
}

/// Parse the indexed-record envelope shared by topology recipe operands.
fn parse_recipe_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    recipe_kind: ConstructionRecipeKind,
    terminator: RecipeOperandTerminator,
) -> Option<ParsedRecipeOperand> {
    let family_name = crate::design::RECIPES
        .iter()
        .find_map(|(name, kind)| (*kind == recipe_kind).then_some(*name))?;
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = Vec::with_capacity(5);
    let mut position = start.checked_add(11)?;
    for record_index in (0..4).map(|delta| header.record_index.checked_add(delta)) {
        let offset = records.first_at_or_after(position, record_index?)?;
        offsets.push(offset);
        position = offset.checked_add(11)?;
    }
    offsets.push(match terminator {
        RecipeOperandTerminator::RecordDelta(delta) => {
            records.first_at_or_after(position, header.record_index.checked_add(delta)?)?
        }
        RecipeOperandTerminator::NextIndexedAfterRecipe { limit } => {
            let recipe_record_byte_offset = u64::try_from(offsets[3]).ok()?;
            let recipe = recipes
                .iter()
                .filter(|recipe| {
                    native_stream(&recipe.id) == Some(stream)
                        && recipe.kind == recipe_kind
                        && recipe.byte_offset > recipe_record_byte_offset
                        && recipe.byte_offset < limit
                })
                .min_by_key(|recipe| recipe.byte_offset)?;
            let recipe_program_at = usize::try_from(recipe.byte_offset)
                .ok()?
                .checked_add(family_name.len())?;
            let next = next_indexed_record_offset(bytes, recipe_program_at)?;
            (u64::try_from(next).ok()? <= limit).then_some(next)?
        }
    });
    let indexed = offsets
        .iter()
        .map(|offset| {
            let (class_tag, after_tag) =
                lp_ascii_filtered(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
            Some((class_tag, View::u32_le_at(bytes, after_tag)?))
        })
        .collect::<Option<Vec<_>>>()?;
    let recipe_record_index = header.record_index.checked_add(3)?;
    let expected_prefix = [
        header.record_index,
        header.record_index.checked_add(1)?,
        header.record_index.checked_add(2)?,
        recipe_record_index,
    ];
    if !indexed
        .iter()
        .take(4)
        .zip(expected_prefix)
        .all(|((_, actual), expected)| *actual == expected)
    {
        return None;
    }
    let next_record_index = indexed[4].1;
    if let RecipeOperandTerminator::RecordDelta(delta) = terminator {
        if next_record_index != header.record_index.checked_add(delta)? {
            return None;
        }
    }
    let recipe_record_byte_offset = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let matches = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && recipe.kind == recipe_kind
                && recipe.byte_offset > recipe_record_byte_offset
                && recipe.byte_offset < next_byte_offset
        })
        .collect::<Vec<_>>();
    let [recipe] = matches.as_slice() else {
        return None;
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name.len(),
    )?;
    let recipe_prefix_offset = u64::try_from(recipe_prefix_at).ok()?;
    let recipe_references = decode_recipe_references(&recipe_prefix_bytes, recipe_prefix_offset);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name.len())?;
    let recipe_program_end = usize::try_from(next_byte_offset).ok()?;
    if recipe_program_end.checked_sub(recipe_program_at)? > 64 * 1024 {
        return None;
    }
    let recipe_program = contiguous_i32_program(bytes, recipe_program_at, recipe_program_end)?;
    Some(ParsedRecipeOperand {
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: indexed[0].0.clone(),
        recipe_record_index,
        recipe_record_byte_offset,
        recipe_id: recipe.id.clone(),
        recipe_prefix_offset,
        recipe_prefix_bytes,
        recipe_references,
        recipe_program_offset: u64::try_from(recipe_program_at).ok()?,
        recipe_program,
        next_record_index,
        next_byte_offset,
    })
}

pub(crate) fn parse_edge_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    terminal_group_limit: Option<u64>,
) -> Option<DesignEdgeOperand> {
    let next_record_delta = edge_recipe_terminal_delta(&scope.kind);
    let stream = native_stream(&scope.id)?;
    let parsed = parse_recipe_operand(
        bytes,
        records,
        stream,
        header,
        recipes,
        ConstructionRecipeKind::Edge,
        RecipeOperandTerminator::RecordDelta(next_record_delta),
    )
    .or_else(|| {
        let limit = terminal_group_limit?;
        parse_recipe_operand(
            bytes,
            records,
            stream,
            header,
            recipes,
            ConstructionRecipeKind::Edge,
            RecipeOperandTerminator::NextIndexedAfterRecipe { limit },
        )
    })?;
    let recipe_structure = edge_recipe_structure(&parsed.recipe_program);
    let surface_patch_recipe_structure = (scope.kind == "SurfacePatch")
        .then(|| {
            surface_patch_recipe_structure(&parsed.recipe_program, parsed.recipe_references.len())
        })
        .flatten();
    let local_topology_references = recipe_structure.as_ref().and_then(|structure| {
        edge_recipe_local_topology_references(structure, parsed.recipe_references.len())
    });
    Some(DesignEdgeOperand {
        id: ids::native_design_edge_operand_id(
            stream.strip_prefix(ids::SCHEME_PREFIX).unwrap_or(stream),
            header.byte_offset,
        ),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: parsed.paired_byte_offset,
        paired_class_tag: parsed.paired_class_tag,
        recipe_record_index: parsed.recipe_record_index,
        recipe_record_byte_offset: parsed.recipe_record_byte_offset,
        recipe_id: parsed.recipe_id,
        recipe_prefix_offset: parsed.recipe_prefix_offset,
        recipe_prefix_bytes: parsed.recipe_prefix_bytes,
        recipe_references: parsed.recipe_references,
        recipe_program_offset: parsed.recipe_program_offset,
        recipe_program: parsed.recipe_program,
        recipe_structure,
        surface_patch_recipe_structure,
        local_topology_references,
        candidate_faces: Vec::new(),
        result_candidate_faces: Vec::new(),
        result_boundary_edge_slots: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        terminal_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        preceding_boundary_edge_slots: Vec::new(),
        terminal_boundary_edge_slots: Vec::new(),
        changed_boundary_edge_slots: Vec::new(),
        deleted_boundary_edge_slots: Vec::new(),
        updated_boundary_edge_slots: Vec::new(),
        treatment_radius_candidates: Vec::new(),
        changed_boundary_edge_contexts: Vec::new(),
        terminal_boundary_edge_contexts: Vec::new(),
        terminal_reference_edge_slots: Vec::new(),
        recipe_reference_contexts: Vec::new(),
        recipe_selectors: Vec::new(),
        recipe_state_id: None,
        resolved_edge_slot: None,
        resolved_axis_origin: None,
        resolved_axis_direction: None,
        next_record_index: parsed.next_record_index,
        next_byte_offset: parsed.next_byte_offset,
    })
}

pub(crate) fn edge_recipe_structure(
    program: &[i32],
) -> Option<crate::records::DesignEdgeRecipeStructure> {
    edge_recipe_structure_tail(program.get(7..)?)
}

/// Decode the alternate two-clause edge-recipe grammar owned by `SurfacePatch`.
///
/// The six fields in each clause are delimiter-bounded by `-1`. The first and
/// second fields name face references, the third and fifth fields name edge
/// references, and the fourth and sixth fields are zero pairs. The final field
/// is a counted sequence of the standard eight-word topology entries.
pub(crate) fn surface_patch_recipe_structure(
    program: &[i32],
    reference_count: usize,
) -> Option<crate::records::DesignSurfacePatchRecipeStructure> {
    let mut remaining = program.get(7..)?;
    let (&root, tail) = remaining.split_first()?;
    if root != 2 {
        return None;
    }
    remaining = tail;
    let mut clauses = Vec::with_capacity(2);
    for _ in 0..2 {
        let mut fields = Vec::with_capacity(6);
        for _ in 0..6 {
            let delimiter_at = remaining.iter().position(|word| *word == -1)?;
            let field = remaining.get(..delimiter_at)?.to_vec();
            if field.is_empty() || field.iter().any(|word| *word < 0) {
                return None;
            }
            remaining = remaining.get(delimiter_at + 1..)?;
            fields.push(field);
        }
        let (&payload_entry_count, tail) = remaining.split_first()?;
        let payload_entry_count = u32::try_from(payload_entry_count).ok()?;
        let payload_word_count = usize::try_from(payload_entry_count).ok()?.checked_mul(8)?;
        let payload = tail.get(..payload_word_count)?;
        let entries = edge_recipe_entries(payload)?;
        if entries.len() != usize::try_from(payload_entry_count).ok()? {
            return None;
        }
        let (&delimiter, tail) = tail.get(payload_word_count..)?.split_first()?;
        if delimiter != -1 {
            return None;
        }
        remaining = tail;
        let [first, second, third, fourth, fifth, sixth] = fields.as_slice() else {
            return None;
        };
        if !(first.len() == 1 || first.len() == 2 && first[0] == 2)
            || second.len() != 1
            || third.len() != 2
            || third[0] != 2
            || fourth.as_slice() != [0, 0]
            || fifth.len() != 1
            || sixth.as_slice() != [0, 0]
        {
            return None;
        }
        let ordinal = |field: &[i32], position: usize| {
            let ordinal = usize::try_from(*field.get(position)?).ok()?;
            (ordinal < reference_count).then_some(u32::try_from(ordinal).ok()?)
        };
        let face_reference_ordinals = [ordinal(first, first.len() - 1)?, ordinal(second, 0)?];
        let edge_reference_ordinals = [ordinal(third, 1)?, ordinal(fifth, 0)?];
        clauses.push(crate::records::DesignSurfacePatchRecipeClause {
            fields,
            face_reference_ordinals,
            edge_reference_ordinals,
            payload_entry_count,
            entries,
        });
    }
    if let Some(&delimiter) = remaining.first() {
        if delimiter != 0 {
            return None;
        }
        remaining = &remaining[1..];
    }
    remaining
        .is_empty()
        .then_some(crate::records::DesignSurfacePatchRecipeStructure { root, clauses })
}

pub(crate) fn edge_recipe_local_topology_references(
    structure: &crate::records::DesignEdgeRecipeStructure,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    topology_recipe_references(
        structure.sides.iter().flat_map(|side| {
            std::iter::once(side.header_value).chain(side.scalars.iter().copied())
        }),
        reference_count,
    )
}

fn edge_recipe_structure_tail(
    program: &[i32],
) -> Option<crate::records::DesignEdgeRecipeStructure> {
    let (&root, mut remaining) = program.split_first()?;
    let side_count = usize::try_from(root).ok()?;
    if side_count == 0 {
        return None;
    }
    remaining = recipe_delimiter(remaining)?;
    let structures = edge_recipe_side_sequences(remaining, side_count)
        .into_iter()
        .filter_map(|(sides, tail)| {
            matches!(tail, [] | [-1 | 0])
                .then_some(crate::records::DesignEdgeRecipeStructure { root, sides })
        })
        .collect::<Vec<_>>();
    let [structure] = structures.as_slice() else {
        return None;
    };
    Some(structure.clone())
}

fn edge_recipe_side_sequences(
    words: &[i32],
    side_count: usize,
) -> Vec<(Vec<DesignTopologyRecipeSide>, &[i32])> {
    if side_count == 0 {
        return vec![(Vec::new(), words)];
    }
    let mut out = Vec::new();
    for (side, tail) in edge_recipe_counted_side_candidates(words) {
        let remaining = if side_count == 1 {
            tail
        } else if let Some(remaining) = recipe_delimiter(tail) {
            remaining
        } else {
            continue;
        };
        for (mut following, tail) in edge_recipe_side_sequences(remaining, side_count - 1) {
            following.insert(0, side.clone());
            out.push((following, tail));
        }
    }
    out
}

fn recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1 | 0)).then(|| &words[1..])
}

fn complete_recipe_payload_prefix(prefix: &[i32]) -> bool {
    if prefix == [0] {
        return true;
    }
    let mut remaining = prefix;
    let mut field_count = 0;
    while !remaining.is_empty() {
        let Some(delimiter_at) = remaining.iter().position(|word| *word == -1) else {
            return false;
        };
        let field = &remaining[..delimiter_at];
        if field.is_empty() || field[0] <= 0 || field.iter().any(|word| *word < 0) {
            return false;
        }
        remaining = &remaining[delimiter_at + 1..];
        if !matches!(remaining.get(..3), Some([0, 0, -1])) {
            return false;
        }
        remaining = &remaining[3..];
        field_count += 1;
    }
    field_count > 0
}

fn edge_recipe_counted_side_candidates(words: &[i32]) -> Vec<(DesignTopologyRecipeSide, &[i32])> {
    let Some(field_count) = words
        .first()
        .and_then(|word| u32::try_from(*word).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Vec::new();
    };
    if field_count.get() < 2 {
        return Vec::new();
    }
    let Some(scalar_count) = usize::try_from(field_count.get())
        .ok()
        .and_then(|count| count.checked_sub(1))
    else {
        return Vec::new();
    };
    let Some(&header_value) = words.get(1) else {
        return Vec::new();
    };
    let Some(mut remaining) = words.get(2..).and_then(recipe_delimiter) else {
        return Vec::new();
    };
    // Each scalar consumes at least one remaining word; a larger count is
    // corrupt and must not reach the allocator.
    if scalar_count > remaining.len() {
        return Vec::new();
    }
    let mut scalars = Vec::with_capacity(scalar_count);
    for _ in 0..scalar_count {
        let Some((&scalar, tail)) = remaining.split_first() else {
            return Vec::new();
        };
        scalars.push(scalar);
        let Some(tail) = recipe_delimiter(tail) else {
            return Vec::new();
        };
        remaining = tail;
    }
    (0..remaining.len())
        .filter(|entry_count_at| {
            let payload_prefix = &remaining[..*entry_count_at];
            complete_recipe_payload_prefix(payload_prefix)
        })
        .filter_map(|entry_count_at| {
            let payload_entry_count = u32::try_from(*remaining.get(entry_count_at)?).ok()?;
            if entry_count_at != 1 && payload_entry_count == 0 {
                return None;
            }
            let payload_len = usize::try_from(payload_entry_count).ok()?.checked_mul(8)?;
            let entries_at = entry_count_at.checked_add(1)?;
            let entries_end = entries_at.checked_add(payload_len)?;
            let entries = edge_recipe_entries(remaining.get(entries_at..entries_end)?)?;
            Some((
                DesignTopologyRecipeSide {
                    field_count,
                    header_value,
                    scalars: scalars.clone(),
                    payload_prefix: remaining[..entry_count_at].to_vec(),
                    payload_entry_count,
                    entries,
                },
                remaining.get(entries_end..)?,
            ))
        })
        .collect()
}

pub(crate) fn face_recipe_structure(
    program: &[i32],
) -> Option<crate::records::DesignFaceRecipeStructure> {
    let (&root, remaining) = program.split_first()?;
    let (&first_prelude, remaining) = recipe_delimiter(remaining)?.split_first()?;
    let (&second_prelude, remaining) = recipe_delimiter(remaining)?.split_first()?;
    let remaining = recipe_delimiter(remaining)?;
    let structures = edge_recipe_side_sequences(remaining, 2)
        .into_iter()
        .filter_map(|(sides, tail)| {
            let postlude = match tail {
                [] | [-1 | 0] => Vec::new(),
                [-1, _, -1, 0, 0, -1] => tail.to_vec(),
                _ => return None,
            };
            Some((sides.try_into().ok()?, postlude))
        })
        .collect::<Vec<([DesignTopologyRecipeSide; 2], Vec<i32>)>>();
    let [(sides, postlude)] = structures.as_slice() else {
        return None;
    };
    Some(crate::records::DesignFaceRecipeStructure {
        root,
        prelude: [first_prelude, second_prelude],
        sides: sides.clone(),
        postlude: postlude.clone(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaceRecipeProgramKind {
    Terminal,
    Counted { header_value: usize },
}

pub(crate) fn face_recipe_program_kind(program: &[i32]) -> Option<FaceRecipeProgramKind> {
    if matches!(program, [0, -1 | 0]) {
        return Some(FaceRecipeProgramKind::Terminal);
    }
    if !matches!(program.get(0..2), Some([0, -1 | 0])) {
        return None;
    }
    let header_value = usize::try_from(*program.get(2)?).ok()?;
    (header_value > 0 && header_value <= 100_000)
        .then_some(FaceRecipeProgramKind::Counted { header_value })
}

fn topology_recipe_references(
    words: impl IntoIterator<Item = i32>,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    words
        .into_iter()
        .filter(|word| *word != 0)
        .map(|word| {
            let ordinal = std::num::NonZeroU32::new(u32::try_from(word).ok()?)?;
            (usize::try_from(ordinal.get()).ok()? <= reference_count).then_some(ordinal)
        })
        .collect()
}

pub(crate) fn edge_recipe_entries(words: &[i32]) -> Option<Vec<DesignTopologyRecipeEntry>> {
    let entries = words
        .chunks_exact(8)
        .map(|entry| {
            let selector = entry[0];
            if selector < 0 {
                return None;
            }
            let boundary_edge_count = std::num::NonZeroU32::new(u32::try_from(entry[1]).ok()?)?;
            let topology_triplets = [
                edge_recipe_topology_triplet(&entry[2..5], boundary_edge_count)?,
                edge_recipe_topology_triplet(&entry[5..8], boundary_edge_count)?,
            ];
            topology_triplets
                .iter()
                .all(|triplet| triplet.outer.get() <= boundary_edge_count.get())
                .then_some(DesignTopologyRecipeEntry {
                    selector,
                    boundary_edge_count,
                    common_incident_edge_ordinal: topology_triplets[0]
                        .incident_edge_ordinal
                        .filter(|ordinal| {
                            topology_triplets[1].incident_edge_ordinal == Some(*ordinal)
                        }),
                    topology_triplets,
                })
        })
        .collect::<Option<Vec<_>>>()?;
    entries
        .windows(2)
        .all(|pair| pair[0].selector < pair[1].selector)
        .then_some(entries)
}

fn edge_recipe_topology_triplet(
    words: &[i32],
    boundary_edge_count: std::num::NonZeroU32,
) -> Option<DesignTopologyRecipeTriplet> {
    let [outer, middle, repeated_outer] = words else {
        return None;
    };
    if outer != repeated_outer {
        return None;
    }
    let outer = std::num::NonZeroU32::new(u32::try_from(*outer).ok()?)?;
    let vertex_ordinal = outer.get().checked_sub(1)?;
    let incident = if *middle == i32::try_from(outer.get()).ok()? {
        Some((
            crate::records::DesignTopologyIncidentSide::Following,
            vertex_ordinal,
        ))
    } else if *middle >= 0 && middle.checked_add(1) == i32::try_from(outer.get()).ok() {
        Some((
            crate::records::DesignTopologyIncidentSide::Preceding,
            vertex_ordinal
                .checked_add(boundary_edge_count.get())?
                .checked_sub(1)?
                % boundary_edge_count.get(),
        ))
    } else {
        None
    };
    Some(DesignTopologyRecipeTriplet {
        outer,
        middle: *middle,
        vertex_ordinal,
        incident_edge_ordinal: incident.map(|(_, ordinal)| ordinal),
        incident_side: incident.map(|(side, _)| side),
    })
}

/// Find the indexed header that terminates a face-recipe member.
///
/// The ordinary envelope terminates at `N+4`. One serialized generation
/// omits that header and terminates at `N+5`. When neither expected index is
/// present within the enclosing boundary, the first following valid indexed
/// header terminates the envelope; its record index has no fixed delta from
/// `N`. Select an expected continuation before applying that fallback.
fn face_recipe_next_boundary(
    bytes: &[u8],
    position: usize,
    record_index: u32,
    limit: Option<u64>,
) -> Option<(usize, u32)> {
    let within_limit = |offset: usize| {
        limit.is_none_or(|limit| {
            usize::try_from(limit)
                .ok()
                .is_some_and(|limit| offset <= limit)
        })
    };
    let mut expected = Vec::with_capacity(2);
    for expected_index in [record_index.checked_add(4)?, record_index.checked_add(5)?] {
        if let Some(offset) = next_indexed_record_offset_with_index(bytes, position, expected_index)
        {
            expected.push((expected_index, offset));
        }
    }
    expected
        .into_iter()
        .filter(|(_, offset)| within_limit(*offset))
        .min_by_key(|(_, offset)| *offset)
        .or_else(|| {
            let offset = next_indexed_record_offset(bytes, position)?;
            let record_index = indexed_record_index(bytes, offset)?;
            within_limit(offset).then_some((record_index, offset))
        })
        .map(|(record_index, offset)| (offset, record_index))
}

// One indexed-offset view rides along with the seven framing inputs the
// parse already required; bundling them would touch every caller for no
// structural gain.
#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_face_operand(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    scope_reference_ordinal: u32,
    group_ownership: Option<(u32, u32)>,
    next_byte_offset: Option<u64>,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<DesignFaceOperand> {
    let start = usize::try_from(header.byte_offset).ok()?;
    let mut offsets = Vec::with_capacity(5);
    let mut position = start.checked_add(11)?;
    for record_index in (0..4).map(|delta| header.record_index.checked_add(delta)) {
        let offset = records.first_at_or_after(position, record_index?)?;
        offsets.push(offset);
        position = offset.checked_add(11)?;
    }
    let (immediate_next, next_record_index) =
        face_recipe_next_boundary(bytes, position, header.record_index, next_byte_offset)?;
    offsets.push(immediate_next);
    let mut indexed = Vec::with_capacity(offsets.len());
    for offset in &offsets {
        let (class_tag, after_tag) =
            lp_ascii_filtered(bytes, *offset, 0..=2000, u8::is_ascii_graphic)?;
        indexed.push((class_tag, View::u32_le_at(bytes, after_tag)?));
    }
    let recipe_record_index = header.record_index.checked_add(3)?;
    if indexed[0].1 != header.record_index
        || indexed[1].1 != header.record_index.checked_add(1)?
        || indexed[2].1 != header.record_index.checked_add(2)?
        || indexed[3].1 != recipe_record_index
        || indexed[4].1 != next_record_index
    {
        return None;
    }
    let stream = native_stream(&scope.id)?;
    let recipe_start = u64::try_from(offsets[3]).ok()?;
    let next_byte_offset = u64::try_from(offsets[4]).ok()?;
    let matches = recipes
        .iter()
        .filter(|recipe| {
            native_stream(&recipe.id) == Some(stream)
                && matches!(
                    recipe.kind,
                    ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
                )
                && recipe.byte_offset > recipe_start
                && recipe.byte_offset < next_byte_offset
        })
        .collect::<Vec<_>>();
    let [recipe] = matches.as_slice() else {
        return None;
    };
    let family_name_len = match recipe.kind {
        ConstructionRecipeKind::Face => b"face_recipe_data".len(),
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data".len(),
        _ => return None,
    };
    let (recipe_prefix_at, recipe_prefix_bytes) = recipe_record_prefix(
        bytes,
        offsets[3],
        usize::try_from(recipe.byte_offset).ok()?,
        family_name_len,
    )?;
    let recipe_references =
        decode_recipe_references(&recipe_prefix_bytes, u64::try_from(recipe_prefix_at).ok()?);
    let recipe_program_at = usize::try_from(recipe.byte_offset)
        .ok()?
        .checked_add(family_name_len)?;
    let recipe_program_end = usize::try_from(next_byte_offset).ok()?;
    if recipe_program_end.checked_sub(recipe_program_at)? > 64 * 1024 {
        return None;
    }
    let recipe_program = contiguous_i32_program(bytes, recipe_program_at, recipe_program_end)?;
    let program_kind = face_recipe_program_kind(&recipe_program)?;
    let recipe_program_offset = u64::try_from(recipe_program_at).ok()?;
    let recipe_node_indices = recipe_program
        .windows(3)
        .enumerate()
        .filter(|(_, values)| *values == [-1, -1, 2])
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if recipe_node_indices.first().is_some_and(|index| *index < 3) {
        return None;
    }
    if program_kind == FaceRecipeProgramKind::Terminal && !recipe_node_indices.is_empty() {
        return None;
    }
    let recipe_node_offsets = recipe_node_indices
        .iter()
        .map(|index| u64::try_from(recipe_program_at.checked_add(index.checked_mul(4)?)?).ok())
        .collect::<Option<Vec<_>>>()?;
    let recipe_nodes = recipe_node_indices
        .iter()
        .copied()
        .zip(
            recipe_node_indices
                .iter()
                .copied()
                .skip(1)
                .chain(std::iter::once(recipe_program.len())),
        )
        .map(|(start, end)| {
            let program = recipe_program.get(start..end)?.to_vec();
            let recipe_structure = program.get(3..).and_then(face_recipe_structure);
            Some(crate::records::DesignFaceRecipeNode {
                byte_offset: u64::try_from(recipe_program_at.checked_add(start.checked_mul(4)?)?)
                    .ok()?,
                end_byte_offset: u64::try_from(recipe_program_at.checked_add(end.checked_mul(4)?)?)
                    .ok()?,
                recipe_structure,
                program,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(DesignFaceOperand {
        id: ids::native_design_face_operand_id(
            stream.strip_prefix(ids::SCHEME_PREFIX).unwrap_or(stream),
            header.byte_offset,
        ),
        scope_record_index: scope.record_index,
        scope_reference_ordinal,
        group: group_ownership.map(|(group_record_index, group_member_ordinal)| {
            crate::records::DesignOperandGroup {
                group_record_index,
                group_member_ordinal,
            }
        }),
        record_index: header.record_index,
        byte_offset: header.byte_offset,
        class_tag: header.class_tag.clone(),
        paired_byte_offset: u64::try_from(offsets[0]).ok()?,
        paired_class_tag: indexed[0].0.clone(),
        recipe_record_index,
        recipe_record_byte_offset: recipe_start,
        recipe_id: recipe.id.clone(),
        recipe_prefix_offset: u64::try_from(recipe_prefix_at).ok()?,
        recipe_prefix_bytes,
        recipe_references,
        recipe_kind: recipe.kind,
        recipe_program_offset,
        recipe_program,
        recipe_node_offsets,
        recipe_nodes,
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index,
        next_byte_offset,
    })
}

pub(crate) fn has_typed_edge_treatment_group(kind: impl AsRef<str>) -> bool {
    matches!(
        design_feature_family(kind),
        Some(DesignFeatureFamily::Fillet | DesignFeatureFamily::Chamfer)
    )
}

/// Apply the selection-identity completeness rule to a parsed group candidate.
///
/// A localized edge-treatment reference table can contain selections that do
/// not use counted groups. Such a reference is a group only when its parsed
/// candidate also resolves through one of the selection-identity grammars.
pub(crate) fn construction_operand_group_is_retained(
    scope_kind: Option<&str>,
    has_selection_identity: bool,
) -> bool {
    !scope_kind.is_some_and(crate::design::is_localized_edge_treatment_kind)
        || has_selection_identity
}

#[cfg(test)]
mod tests;
