// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Autodesk Fusion native design and construction-history records.

use std::collections::HashMap;

#[cfg(test)]
thread_local! {
    static LOAD_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
/// Reset the per-thread typed-native load counter used by writer tests.
pub(crate) fn reset_load_count() {
    LOAD_COUNT.set(0);
}

#[cfg(test)]
/// Number of typed-native loads performed on this test thread since reset.
pub(crate) fn load_count() -> usize {
    LOAD_COUNT.get()
}

use serde::{Deserialize, Serialize};

use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology,
    AsmHistoricalTransition, AsmHistory,
};
use crate::records::dimension_locus_arenas::{
    DesignDimensionLocusPairs, DesignDimensionNullLocusPairs,
};
use crate::records::feature::{
    DesignComponentOccurrence, DesignEdgeTreatmentVertexOperand, DesignParameterScope,
    DesignSurfaceTrimOperation,
};
use crate::records::topology::{
    DesignBodyRecipeOperand, DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
    DesignEdgeIdentityOperand, DesignEdgeOperand, DesignEntitySelectionOperand,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignFaceOperand,
    DesignFaceSourceGroup, DesignFilletRadiusGroup, DesignLoftLegacyBodyCarrier,
};
use crate::records::{
    ActEntity, ActGuid, ActRegistryChannel, ActRootComponent, ActTableReference, BodyVisibility,
    ConstructionRecipe, CreationTimestamp, DesignBodyBinding, DesignBodyBounds, DesignBodyMember,
    DesignCanvasImage, DesignComponentNamingSpace, DesignConfiguration, DesignDecalImage,
    DesignDimensionAnnotationFrame, DesignDimensionLocusGroup, DesignDimensionPresentationFrame,
    DesignDimensionRecipeRecord, DesignEntityHeader, DesignFeatureTimeline,
    DesignMaterialAssignment, DesignMeshFeature, DesignParameter, DesignParameterCompanion,
    DesignParameterOwner, DesignRecordHeader, DesignSketchPlacement, LostEdgeReference,
    PersistentDesignLink, PersistentReference, PersistentSubentityTag, SegmentType,
    SketchCurveIdentity, SketchCurveLink, SketchPoint, SketchRelation, SketchSurface, SketchText,
    XrefDesign, XrefReference,
};
use cadmpeg_asm::brep::records::{
    BodyNativeKey, EdgeContinuity, EdgeOwnership, FaceNativeKey, FaceSidedness,
    MeshSurfaceSentinel, TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail,
    TransformHints, VertexOwnership, WireTopology,
};

fn owner_indices<'a>(ids: impl IntoIterator<Item = &'a str>) -> HashMap<String, usize> {
    ids.into_iter()
        .enumerate()
        .map(|(ordinal, id)| (id.to_owned(), ordinal))
        .collect()
}

fn group_by_owner<T>(
    records: Vec<T>,
    owners: &HashMap<String, usize>,
    owner_count: usize,
    owner: impl Fn(&T) -> &str,
) -> Vec<Vec<T>> {
    let mut grouped = std::iter::repeat_with(Vec::new)
        .take(owner_count)
        .collect::<Vec<_>>();
    for record in records {
        if let Some(&ordinal) = owners.get(owner(&record)) {
            grouped[ordinal].push(record);
        }
    }
    grouped
}

pub(crate) const F3D_ARENA_NAMES: &[&str] = &[
    "act_entities",
    "act_guids",
    "act_registry_channels",
    "act_root_components",
    "act_table_references",
    "asm_bulletin_boards",
    "asm_delta_states",
    "asm_entity_changes",
    "asm_histories",
    "asm_history_records",
    "body_native_keys",
    "body_visibilities",
    "construction_recipes",
    "creation_timestamps",
    "design_body_bindings",
    "design_body_bounds",
    "design_body_members",
    "design_body_recipe_operands",
    "design_canvas_images",
    "design_component_naming_spaces",
    "design_component_occurrences",
    "design_configurations",
    "design_construction_operand_groups",
    "design_construction_operand_identities",
    "design_decal_images",
    "design_dimension_annotation_frames",
    "design_dimension_locus_groups",
    "design_dimension_locus_pairs",
    "design_dimension_null_locus_pairs",
    "design_dimension_presentation_frames",
    "design_dimension_recipe_records",
    "design_edge_identity_operands",
    "design_edge_operands",
    "design_edge_treatment_vertex_operands",
    "design_entity_headers",
    "design_entity_selection_operands",
    "design_extrude_selection_groups",
    "design_extrude_selection_members",
    "design_face_operands",
    "design_face_source_groups",
    "design_feature_timelines",
    "design_fillet_radius_groups",
    "design_loft_legacy_body_carriers",
    "design_material_assignments",
    "design_mesh_features",
    "design_parameter_companions",
    "design_parameter_owners",
    "design_parameter_scopes",
    "design_parameters",
    "design_record_headers",
    "design_sketch_placements",
    "design_surface_trim_operations",
    "design_types",
    "edge_continuities",
    "edge_ownerships",
    "face_native_keys",
    "face_sidedness",
    "lost_edge_references",
    "mesh_surface_sentinels",
    "persistent_design_links",
    "persistent_references",
    "persistent_subentity_tags",
    "sketch_curve_identities",
    "sketch_curve_links",
    "sketch_points",
    "sketch_relations",
    "sketch_surfaces",
    "sketch_texts",
    "tolerant_coedge_parameters",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "transform_hints",
    "vertex_ownerships",
    "wire_topologies",
    "xref_designs",
    "xref_references",
];

type F3dFamilyRow = FamilyRow<F3dNative, (), cadmpeg_ir::NativeNamespace, ()>;

#[derive(Serialize)]
struct FlatAsmHistory<'a> {
    id: &'a str,
    byte_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history_entry_count: Option<i64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    record_table_binding_budget_exceeded: bool,
    states: EmptyList,
}

impl<'a> From<&'a AsmHistory> for FlatAsmHistory<'a> {
    fn from(history: &'a AsmHistory) -> Self {
        Self {
            id: &history.id,
            byte_offset: history.byte_offset,
            stream_size: history.stream_size(),
            history_entry_count: history.history_entry_count(),
            record_table_binding_budget_exceeded: history.record_table_binding_budget_exceeded,
            states: EmptyList,
        }
    }
}

#[derive(Serialize)]
struct FlatAsmDeltaState<'a> {
    id: &'a str,
    parent: &'a str,
    byte_offset: u64,
    state_id: i64,
    version_flag: i64,
    state_flag: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_ref: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_ref: Option<i64>,
    node_index: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    partner_ref: Option<i64>,
    owner_ref: i64,
    bulletin_boards: EmptyList,
    records: EmptyList,
    #[serde(skip_serializing_if = "slice_is_empty")]
    entity_versions: &'a [AsmEntityVersion],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    record_table_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    topology: Option<&'a AsmHistoricalTopology>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transition: Option<&'a AsmHistoricalTransition>,
}

impl<'a> From<&'a AsmDeltaState> for FlatAsmDeltaState<'a> {
    fn from(state: &'a AsmDeltaState) -> Self {
        Self {
            id: &state.id,
            parent: &state.parent,
            byte_offset: state.byte_offset,
            state_id: state.state_id,
            version_flag: state.version_flag,
            state_flag: state.state_flag,
            previous_ref: state.previous_ref,
            next_ref: state.next_ref,
            node_index: state.node_index,
            partner_ref: state.partner_ref,
            owner_ref: state.owner_ref,
            bulletin_boards: EmptyList,
            records: EmptyList,
            entity_versions: &state.entity_versions,
            record_table_complete: state.record_table_complete(),
            topology: state.topology(),
            transition: state.transition.as_ref(),
        }
    }
}

#[derive(Serialize)]
struct FlatAsmBulletinBoard<'a> {
    id: &'a str,
    parent: &'a str,
    byte_offset: u64,
    owner_ref: i64,
    number: i64,
    changes: EmptyList,
}

impl<'a> From<&'a AsmBulletinBoard> for FlatAsmBulletinBoard<'a> {
    fn from(board: &'a AsmBulletinBoard) -> Self {
        Self {
            id: &board.id,
            parent: &board.parent,
            byte_offset: board.byte_offset,
            owner_ref: board.owner_ref,
            number: board.number,
            changes: EmptyList,
        }
    }
}

/// A nested collection the flat projection always emits empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EmptyList;

impl Serialize for EmptyList {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(std::iter::empty::<()>())
    }
}

fn slice_is_empty<T>(slice: &&[T]) -> bool {
    slice.is_empty()
}

fn emit_asm_histories(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_arena_from(
        row.arena,
        model.asm_histories.iter().map(FlatAsmHistory::from),
    )
}

fn emit_asm_delta_states(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_arena_from(
        row.arena,
        model
            .asm_histories
            .iter()
            .flat_map(|history| &history.states)
            .map(FlatAsmDeltaState::from),
    )
}

fn emit_asm_bulletin_boards(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_arena_from(
        row.arena,
        model
            .asm_histories
            .iter()
            .flat_map(|history| &history.states)
            .flat_map(|state| &state.bulletin_boards)
            .map(FlatAsmBulletinBoard::from),
    )
}

fn emit_asm_entity_changes(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let records = model
        .asm_histories
        .iter()
        .flat_map(|history| &history.states)
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes);
    namespace.set_arena_from(row.arena, records)
}

fn emit_asm_history_records(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let records = model
        .asm_histories
        .iter()
        .flat_map(|history| &history.states)
        .flat_map(|state| &state.records);
    namespace.set_arena_from(row.arena, records)
}

pub(crate) const F3D_FAMILIES: &[F3dFamilyRow] = &[
    F3dFamilyRow {
        arena: "act_entities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_entities),
        len: |model| model.act_entities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_guids",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_guids),
        len: |model| model.act_guids.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_registry_channels",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_registry_channels),
        len: |model| model.act_registry_channels.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_root_components",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_root_components),
        len: |model| model.act_root_components.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_table_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_table_references),
        len: |model| model.act_table_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "body_native_keys",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.body_native_keys),
        len: |model| model.body_native_keys.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "body_visibilities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.body_visibilities),
        len: |model| model.body_visibilities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_types",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_types),
        len: |model| model.design_types.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_canvas_images",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_canvas_images),
        len: |model| model.design_canvas_images.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_decal_images",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_decal_images),
        len: |model| model.design_decal_images.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_mesh_features",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_mesh_features),
        len: |model| model.design_mesh_features.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_component_occurrences",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_component_occurrences)
        },
        len: |model| model.design_component_occurrences.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_component_naming_spaces",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_component_naming_spaces)
        },
        len: |model| model.design_component_naming_spaces.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_recipe_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_body_recipe_operands)
        },
        len: |model| model.design_body_recipe_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_loft_legacy_body_carriers",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_loft_legacy_body_carriers)
        },
        len: |model| model.design_loft_legacy_body_carriers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_annotation_frames",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_annotation_frames)
        },
        len: |model| model.design_dimension_annotation_frames.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_locus_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_locus_groups)
        },
        len: |model| model.design_dimension_locus_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_locus_pairs",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_locus_pairs)
        },
        len: |model| model.design_dimension_locus_pairs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_null_locus_pairs",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena_from(
                row.arena,
                model
                    .design_dimension_null_locus_pairs
                    .iter()
                    .map(crate::records::dimension_null_locus_wire::Wire::from),
            )
        },
        len: |model| model.design_dimension_null_locus_pairs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_presentation_frames",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_presentation_frames)
        },
        len: |model| model.design_dimension_presentation_frames.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_recipe_records",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_recipe_records)
        },
        len: |model| model.design_dimension_recipe_records.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_edge_operands),
        len: |model| model.design_edge_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_treatment_vertex_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_edge_treatment_vertex_operands)
        },
        len: |model| model.design_edge_treatment_vertex_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_identity_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_edge_identity_operands)
        },
        len: |model| model.design_edge_identity_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_entity_selection_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_entity_selection_operands)
        },
        len: |model| model.design_entity_selection_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_face_operands",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_face_operands),
        len: |model| model.design_face_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_face_source_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_face_source_groups)
        },
        len: |model| model.design_face_source_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_construction_operand_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_construction_operand_groups)
        },
        len: |model| model.design_construction_operand_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_construction_operand_identities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_construction_operand_identities)
        },
        len: |model| model.design_construction_operand_identities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_extrude_selection_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_extrude_selection_groups)
        },
        len: |model| model.design_extrude_selection_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_extrude_selection_members",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_extrude_selection_members)
        },
        len: |model| model.design_extrude_selection_members.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_feature_timelines",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_feature_timelines)
        },
        len: |model| model.design_feature_timelines.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_fillet_radius_groups",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_fillet_radius_groups)
        },
        len: |model| model.design_fillet_radius_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_companions",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_parameter_companions)
        },
        len: |model| model.design_parameter_companions.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_owners",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_parameter_owners)
        },
        len: |model| model.design_parameter_owners.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_scopes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_parameter_scopes)
        },
        len: |model| model.design_parameter_scopes.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_surface_trim_operations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_surface_trim_operations)
        },
        len: |model| model.design_surface_trim_operations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameters",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_parameters),
        len: |model| model.design_parameters.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_entity_headers",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_entity_headers),
        len: |model| model.design_entity_headers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_record_headers",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_record_headers),
        len: |model| model.design_record_headers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_sketch_placements",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_sketch_placements)
        },
        len: |model| model.design_sketch_placements.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_bindings",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_body_bindings),
        len: |model| model.design_body_bindings.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_bounds",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_body_bounds),
        len: |model| model.design_body_bounds.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_members",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_body_members),
        len: |model| model.design_body_members.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_configurations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_configurations),
        len: |model| model.design_configurations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_material_assignments",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_material_assignments)
        },
        len: |model| model.design_material_assignments.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "edge_continuities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.edge_continuities),
        len: |model| model.edge_continuities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "edge_ownerships",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.edge_ownerships),
        len: |model| model.edge_ownerships.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "face_sidedness",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.face_sidedness),
        len: |model| model.face_sidedness.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "face_native_keys",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.face_native_keys),
        len: |model| model.face_native_keys.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "construction_recipes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.construction_recipes),
        len: |model| model.construction_recipes.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "creation_timestamps",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.creation_timestamps),
        len: |model| model.creation_timestamps.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_design_links",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.persistent_design_links)
        },
        len: |model| model.persistent_design_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.persistent_references),
        len: |model| model.persistent_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_subentity_tags",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.persistent_subentity_tags)
        },
        len: |model| model.persistent_subentity_tags.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_curve_links",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_curve_links),
        len: |model| model.sketch_curve_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_relations",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_relations),
        len: |model| model.sketch_relations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_points",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_points),
        len: |model| model.sketch_points.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_curve_identities",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.sketch_curve_identities)
        },
        len: |model| model.sketch_curve_identities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_surfaces",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_surfaces),
        len: |model| model.sketch_surfaces.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_texts",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_texts),
        len: |model| model.sketch_texts.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "lost_edge_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.lost_edge_references),
        len: |model| model.lost_edge_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "mesh_surface_sentinels",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.mesh_surface_sentinels),
        len: |model| model.mesh_surface_sentinels.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "vertex_ownerships",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.vertex_ownerships),
        len: |model| model.vertex_ownerships.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_coedge_parameters",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.tolerant_coedge_parameters)
        },
        len: |model| model.tolerant_coedge_parameters.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_edge_tails",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.tolerant_edge_tails),
        len: |model| model.tolerant_edge_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_vertex_tails",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.tolerant_vertex_tails),
        len: |model| model.tolerant_vertex_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "transform_hints",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.transform_hints),
        len: |model| model.transform_hints.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "wire_topologies",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.wire_topologies),
        len: |model| model.wire_topologies.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "xref_designs",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.xref_designs),
        len: |model| model.xref_designs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "xref_references",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.xref_references),
        len: |model| model.xref_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_histories",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_histories,
        len: |model| model.asm_histories.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_delta_states",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_delta_states,
        len: |model| {
            model
                .asm_histories
                .iter()
                .map(|history| history.states.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_bulletin_boards",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_bulletin_boards,
        len: |model| {
            model
                .asm_histories
                .iter()
                .flat_map(|history| &history.states)
                .map(|state| state.bulletin_boards.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_entity_changes",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_entity_changes,
        len: |model| {
            model
                .asm_histories
                .iter()
                .flat_map(|history| &history.states)
                .flat_map(|state| &state.bulletin_boards)
                .map(|board| board.changes.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_history_records",
        exactness: (),
        phase: Phase::ArenaOnly,
        emit: emit_asm_history_records,
        len: |model| {
            model
                .asm_histories
                .iter()
                .flat_map(|history| &history.states)
                .map(|state| state.records.len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
];

const F3D_CATALOGUE: Catalogue<'static, F3dNative, (), cadmpeg_ir::NativeNamespace, ()> =
    Catalogue::new(F3D_FAMILIES);

/// Autodesk Fusion records retained outside the format-neutral model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct F3dNative {
    /// Fusion ACT change-tracking table entities.
    #[serde(default)]
    pub act_entities: Vec<ActEntity>,
    /// Fusion ACT stream-wide asset/change-version GUID pool.
    #[serde(default)]
    pub act_guids: Vec<ActGuid>,
    /// Fusion ACT stream-wide named channel registry.
    #[serde(default)]
    pub act_registry_channels: Vec<ActRegistryChannel>,
    /// Fusion ACT document-root-to-registry links.
    #[serde(default)]
    pub act_root_components: Vec<ActRootComponent>,
    /// Fusion ACT table references between the GUID pool and channel registry.
    #[serde(default)]
    pub act_table_references: Vec<ActTableReference>,
    /// Native Design-join keys stored on ASM bodies.
    #[serde(default)]
    pub body_native_keys: Vec<BodyNativeKey>,
    /// Design browser-node visibility joined to solved ASM bodies.
    #[serde(default)]
    pub body_visibilities: Vec<BodyVisibility>,
    /// Design `MetaStream` type-table entries.
    #[serde(default)]
    pub design_types: Vec<SegmentType>,
    /// Whole-body operands joined to persistent body construction recipes.
    #[serde(default)]
    pub design_body_recipe_operands: Vec<DesignBodyRecipeOperand>,
    /// Exact role-less body carriers paired with legacy Boolean-Loft groups.
    #[serde(default)]
    pub design_loft_legacy_body_carriers: Vec<DesignLoftLegacyBodyCarrier>,
    /// Exact image-plane bindings owned by Canvas timeline objects.
    #[serde(default)]
    pub design_canvas_images: Vec<DesignCanvasImage>,
    /// Exact image and target bindings owned by Decal timeline objects.
    #[serde(default)]
    pub design_decal_images: Vec<DesignDecalImage>,
    /// Complete typed `Base Mesh Feature` Design graphs.
    #[serde(default)]
    pub design_mesh_features: Vec<DesignMeshFeature>,
    /// Exact local component-definition and placed-occurrence carriers.
    #[serde(default)]
    pub design_component_occurrences: Vec<DesignComponentOccurrence>,
    /// Component-local entity naming spaces selected by context UUID.
    #[serde(default)]
    pub design_component_naming_spaces: Vec<DesignComponentNamingSpace>,
    /// Annotated paired dimension frames governing parameter companions.
    #[serde(default)]
    pub design_dimension_annotation_frames: Vec<DesignDimensionAnnotationFrame>,
    /// Fusion presentation frames governing parameter companions.
    #[serde(default)]
    pub design_dimension_presentation_frames: Vec<DesignDimensionPresentationFrame>,
    /// Typed paired loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_locus_pairs: DesignDimensionLocusPairs,
    /// Counted typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_locus_groups: Vec<DesignDimensionLocusGroup>,
    /// Null-plus-typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_null_locus_pairs: DesignDimensionNullLocusPairs,
    /// Indexed records containing dimension-owned construction recipes.
    #[serde(default)]
    pub design_dimension_recipe_records: Vec<DesignDimensionRecipeRecord>,
    /// Edge-selection operands recovered from Fillet and Chamfer scopes.
    #[serde(default)]
    pub design_edge_operands: Vec<DesignEdgeOperand>,
    /// Corner-vertex operands recovered from edge-treatment groups.
    #[serde(default)]
    pub design_edge_treatment_vertex_operands: Vec<DesignEdgeTreatmentVertexOperand>,
    /// Persistent selection identities recovered from Fillet and Chamfer groups.
    #[serde(default)]
    pub design_edge_identity_operands: Vec<DesignEdgeIdentityOperand>,
    /// Face-selection operands recovered from Extrude construction groups.
    #[serde(default)]
    pub design_face_operands: Vec<DesignFaceOperand>,
    /// Ordered persistent source identities recovered from Face operations.
    #[serde(default)]
    pub design_face_source_groups: Vec<DesignFaceSourceGroup>,
    /// Counted Design scope lists in authored feature order.
    #[serde(default)]
    pub design_feature_timelines: Vec<DesignFeatureTimeline>,
    /// Counted construction-operand groups owned by feature parameter scopes.
    #[serde(default)]
    pub design_construction_operand_groups: Vec<DesignConstructionOperandGroup>,
    /// Persistent identity frames named by construction-operand groups.
    #[serde(default)]
    pub design_construction_operand_identities: Vec<DesignConstructionOperandIdentity>,
    /// Counted selection groups owned by Extrude parameter scopes.
    #[serde(default)]
    pub design_extrude_selection_groups: Vec<DesignExtrudeSelectionGroup>,
    /// Fixed-width members named by Extrude selection groups.
    #[serde(default)]
    pub design_extrude_selection_members: Vec<DesignExtrudeSelectionMember>,
    /// Nested persistent-entity operands named by counted construction groups.
    #[serde(default)]
    pub design_entity_selection_operands: Vec<DesignEntitySelectionOperand>,
    /// Radius parameters paired with counted Fillet edge groups.
    #[serde(default)]
    pub design_fillet_radius_groups: Vec<DesignFilletRadiusGroup>,
    /// Fixed prefixes of indexed records paired with parameter owners.
    #[serde(default)]
    pub design_parameter_companions: Vec<DesignParameterCompanion>,
    /// Same-index-delimited owner frames for indexed Design parameters.
    #[serde(default)]
    pub design_parameter_owners: Vec<DesignParameterOwner>,
    /// Sketch and construction-operation records that scope parameters.
    #[serde(default)]
    pub design_parameter_scopes: Vec<DesignParameterScope>,
    /// Exact BRep-cell carriers owned by `SurfaceTrim` operations.
    #[serde(default)]
    pub design_surface_trim_operations: Vec<DesignSurfaceTrimOperation>,
    /// Indexed Design parameter and expression records.
    #[serde(default)]
    pub design_parameters: Vec<DesignParameter>,
    /// Local-to-model placement frames for Design sketches.
    #[serde(default)]
    pub design_sketch_placements: Vec<DesignSketchPlacement>,
    /// Self-validating per-entity headers from the Design `BulkStream`.
    #[serde(default)]
    pub design_entity_headers: Vec<DesignEntityHeader>,
    /// Indexed dynamic-class record headers from the Design `BulkStream`.
    #[serde(default)]
    pub design_record_headers: Vec<DesignRecordHeader>,
    /// `BodiesRoot` list members from the Design `BulkStream`.
    #[serde(default)]
    pub design_body_members: Vec<DesignBodyMember>,
    /// Ordered BREP body-map pairs from Design streams.
    #[serde(default)]
    pub design_body_bindings: Vec<DesignBodyBinding>,
    /// Triplicated axis-aligned bounds cached by Design body containers.
    #[serde(default)]
    pub design_body_bounds: Vec<DesignBodyBounds>,
    /// Design configuration tables and rules with complete JSON payloads.
    #[serde(default)]
    pub design_configurations: Vec<DesignConfiguration>,
    /// Design entity-to-material assignment records.
    #[serde(default)]
    pub design_material_assignments: Vec<DesignMaterialAssignment>,
    /// Kernel continuity classifications stored on solved ASM edges.
    #[serde(default)]
    pub edge_continuities: Vec<EdgeContinuity>,
    /// Native owner-coedge selectors stored on ASM edges.
    #[serde(default)]
    pub edge_ownerships: Vec<EdgeOwnership>,
    /// Native single/double-sided classifications stored on ASM faces.
    #[serde(default)]
    pub face_sidedness: Vec<FaceSidedness>,
    /// Native Design-join keys stored on solved ASM faces.
    #[serde(default)]
    pub face_native_keys: Vec<FaceNativeKey>,
    /// Parametric regeneration recipes from the Design `BulkStream`.
    #[serde(default)]
    pub construction_recipes: Vec<ConstructionRecipe>,
    /// Original authoring times attached to solved entities.
    #[serde(default)]
    pub creation_timestamps: Vec<CreationTimestamp>,
    /// Persistent Fusion design identifiers attached to solved B-rep entities.
    #[serde(default)]
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Persistent point/curve references from Design construction records.
    #[serde(default)]
    pub persistent_references: Vec<PersistentReference>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    #[serde(default)]
    pub persistent_subentity_tags: Vec<PersistentSubentityTag>,
    /// Provenance links from sketch curves to generated B-rep coedges.
    #[serde(default)]
    pub sketch_curve_links: Vec<SketchCurveLink>,
    /// Bidirectional relations owned by sketch containers.
    #[serde(default)]
    pub sketch_relations: Vec<SketchRelation>,
    /// Persistent source sketch points.
    #[serde(default)]
    pub sketch_points: Vec<SketchPoint>,
    /// Persistent identity pairs attached to source sketch-curve records.
    #[serde(default)]
    pub sketch_curve_identities: Vec<SketchCurveIdentity>,
    /// Persistent tensor-product surfaces owned by spatial sketches.
    #[serde(default)]
    pub sketch_surfaces: Vec<SketchSurface>,
    /// Persistent text entities owned by planar sketches.
    #[serde(default)]
    pub sketch_texts: Vec<SketchText>,
    /// Construction-history edge selections that Fusion could not re-resolve.
    #[serde(default)]
    pub lost_edge_references: Vec<LostEdgeReference>,
    /// Zero-payload ASM mesh-surface sentinels linked to unknown exact surfaces.
    #[serde(default)]
    pub mesh_surface_sentinels: Vec<MeshSurfaceSentinel>,
    /// Native owner-edge and endpoint-slot fields stored on ASM vertices.
    #[serde(default)]
    pub vertex_ownerships: Vec<VertexOwnership>,
    /// Native parameter intervals stored on tolerant ASM coedges.
    #[serde(default)]
    pub tolerant_coedge_parameters: Vec<TolerantCoedgeParameters>,
    /// Native trailing LONG slots stored on tolerant ASM edges.
    #[serde(default)]
    pub tolerant_edge_tails: Vec<TolerantEdgeTail>,
    /// Native trailing f32 slots stored on tolerant ASM vertices.
    #[serde(default)]
    pub tolerant_vertex_tails: Vec<TolerantVertexTail>,
    /// Native transform rotation/reflection/shear classifications.
    #[serde(default)]
    pub transform_hints: Vec<TransformHints>,
    /// Native wire records and their side classifications.
    #[serde(default)]
    pub wire_topologies: Vec<WireTopology>,
    /// Container external-reference design entries
    /// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
    #[serde(default)]
    pub xref_designs: Vec<XrefDesign>,
    /// Container outgoing XREF placements
    /// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
    #[serde(default)]
    pub xref_references: Vec<XrefReference>,
    /// ASM construction-history containers and their linked delta states.
    #[serde(default)]
    pub asm_histories: Vec<AsmHistory>,
}

impl F3dNative {
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        #[cfg(test)]
        LOAD_COUNT.set(LOAD_COUNT.get() + 1);
        let mut native = Self {
            act_entities: namespace.arena_as("act_entities")?,
            act_guids: namespace.arena_as("act_guids")?,
            act_registry_channels: namespace.arena_as("act_registry_channels")?,
            act_root_components: namespace.arena_as("act_root_components")?,
            act_table_references: namespace.arena_as("act_table_references")?,
            body_native_keys: namespace.arena_as("body_native_keys")?,
            body_visibilities: namespace.arena_as("body_visibilities")?,
            design_types: namespace.arena_as("design_types")?,
            design_canvas_images: namespace.arena_as("design_canvas_images")?,
            design_decal_images: namespace.arena_as("design_decal_images")?,
            design_mesh_features: namespace.arena_as("design_mesh_features")?,
            design_component_occurrences: namespace.arena_as("design_component_occurrences")?,
            design_component_naming_spaces: namespace.arena_as("design_component_naming_spaces")?,
            design_body_recipe_operands: namespace.arena_as("design_body_recipe_operands")?,
            design_loft_legacy_body_carriers: namespace
                .arena_as("design_loft_legacy_body_carriers")?,
            design_dimension_annotation_frames: namespace
                .arena_as("design_dimension_annotation_frames")?,
            design_dimension_presentation_frames: namespace
                .arena_as("design_dimension_presentation_frames")?,
            design_dimension_locus_groups: namespace.arena_as("design_dimension_locus_groups")?,
            design_dimension_locus_pairs: DesignDimensionLocusPairs::try_from(
                namespace.arena_as::<crate::records::DesignDimensionLocusPair>(
                    "design_dimension_locus_pairs",
                )?,
            )
            .map_err(<serde_json::Error as serde::de::Error>::custom)?,
            design_dimension_null_locus_pairs: DesignDimensionNullLocusPairs::try_from(
                namespace.arena_as::<crate::records::dimension_null_locus_wire::Entry>(
                    "design_dimension_null_locus_pairs",
                )?,
            )
            .map_err(<serde_json::Error as serde::de::Error>::custom)?,
            design_dimension_recipe_records: namespace
                .arena_as("design_dimension_recipe_records")?,
            design_edge_operands: namespace.arena_as("design_edge_operands")?,
            design_edge_treatment_vertex_operands: namespace
                .arena_as("design_edge_treatment_vertex_operands")?,
            design_edge_identity_operands: namespace.arena_as("design_edge_identity_operands")?,
            design_entity_selection_operands: namespace
                .arena_as("design_entity_selection_operands")?,
            design_face_operands: namespace.arena_as("design_face_operands")?,
            design_face_source_groups: namespace.arena_as("design_face_source_groups")?,
            design_feature_timelines: namespace.arena_as("design_feature_timelines")?,
            design_construction_operand_groups: namespace
                .arena_as("design_construction_operand_groups")?,
            design_construction_operand_identities: namespace
                .arena_as("design_construction_operand_identities")?,
            design_extrude_selection_groups: namespace
                .arena_as("design_extrude_selection_groups")?,
            design_extrude_selection_members: namespace
                .arena_as("design_extrude_selection_members")?,
            design_fillet_radius_groups: namespace.arena_as("design_fillet_radius_groups")?,
            design_parameter_companions: namespace.arena_as("design_parameter_companions")?,
            design_parameter_owners: namespace.arena_as("design_parameter_owners")?,
            design_parameter_scopes: namespace.arena_as("design_parameter_scopes")?,
            design_surface_trim_operations: namespace.arena_as("design_surface_trim_operations")?,
            design_parameters: namespace.arena_as("design_parameters")?,
            design_entity_headers: namespace.arena_as("design_entity_headers")?,
            design_record_headers: namespace.arena_as("design_record_headers")?,
            design_sketch_placements: namespace.arena_as("design_sketch_placements")?,
            design_body_bindings: namespace.arena_as("design_body_bindings")?,
            design_body_bounds: namespace.arena_as("design_body_bounds")?,
            design_body_members: namespace.arena_as("design_body_members")?,
            design_configurations: namespace.arena_as("design_configurations")?,
            design_material_assignments: namespace.arena_as("design_material_assignments")?,
            edge_continuities: namespace.arena_as("edge_continuities")?,
            edge_ownerships: namespace.arena_as("edge_ownerships")?,
            face_sidedness: namespace.arena_as("face_sidedness")?,
            face_native_keys: namespace.arena_as("face_native_keys")?,
            construction_recipes: namespace.arena_as("construction_recipes")?,
            creation_timestamps: namespace.arena_as("creation_timestamps")?,
            persistent_design_links: namespace.arena_as("persistent_design_links")?,
            persistent_references: namespace.arena_as("persistent_references")?,
            persistent_subentity_tags: namespace.arena_as("persistent_subentity_tags")?,
            sketch_curve_links: namespace.arena_as("sketch_curve_links")?,
            sketch_relations: namespace.arena_as("sketch_relations")?,
            sketch_points: namespace.arena_as("sketch_points")?,
            sketch_curve_identities: namespace.arena_as("sketch_curve_identities")?,
            sketch_surfaces: namespace.arena_as("sketch_surfaces")?,
            sketch_texts: namespace.arena_as("sketch_texts")?,
            lost_edge_references: namespace.arena_as("lost_edge_references")?,
            mesh_surface_sentinels: namespace.arena_as("mesh_surface_sentinels")?,
            vertex_ownerships: namespace.arena_as("vertex_ownerships")?,
            tolerant_coedge_parameters: namespace.arena_as("tolerant_coedge_parameters")?,
            tolerant_edge_tails: namespace.arena_as("tolerant_edge_tails")?,
            tolerant_vertex_tails: namespace.arena_as("tolerant_vertex_tails")?,
            transform_hints: namespace.arena_as("transform_hints")?,
            wire_topologies: namespace.arena_as("wire_topologies")?,
            xref_designs: namespace.arena_as("xref_designs")?,
            xref_references: namespace.arena_as("xref_references")?,
            asm_histories: namespace.arena_as("asm_histories")?,
        };
        let states: Vec<crate::history_records::AsmDeltaState> =
            namespace.arena_as("asm_delta_states")?;
        let boards: Vec<crate::history_records::AsmBulletinBoard> =
            namespace.arena_as("asm_bulletin_boards")?;
        let changes: Vec<crate::history_records::AsmEntityChange> =
            namespace.arena_as("asm_entity_changes")?;
        let records: Vec<crate::history_records::AsmHistoryRecord> =
            namespace.arena_as("asm_history_records")?;
        let board_indices = owner_indices(boards.iter().map(|board| board.id.as_str()));
        let changes_by_board = group_by_owner(changes, &board_indices, boards.len(), |change| {
            &change.parent
        });
        let boards = boards
            .into_iter()
            .zip(changes_by_board)
            .map(|(mut board, changes)| {
                board.changes = changes;
                board
            })
            .collect::<Vec<_>>();
        let state_indices = owner_indices(states.iter().map(|state| state.id.as_str()));
        let boards_by_state =
            group_by_owner(boards, &state_indices, states.len(), |board| &board.parent);
        let records_by_state = group_by_owner(records, &state_indices, states.len(), |record| {
            &record.parent
        });
        let states = states
            .into_iter()
            .zip(boards_by_state)
            .zip(records_by_state)
            .map(|((mut state, bulletin_boards), records)| {
                state.bulletin_boards = bulletin_boards;
                state.records = records;
                state
            })
            .collect::<Vec<_>>();
        let history_indices = owner_indices(
            native
                .asm_histories
                .iter()
                .map(|history| history.id.as_str()),
        );
        let states_by_history = group_by_owner(
            states,
            &history_indices,
            native.asm_histories.len(),
            |state| &state.parent,
        );
        for (history, states) in native.asm_histories.iter_mut().zip(states_by_history) {
            history.states = states;
        }
        Ok(native)
    }

    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        F3D_CATALOGUE.emit_all(self, namespace)?;
        debug_assert!(F3D_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas().contains_key(*name)));
        Ok(())
    }
}

#[cfg(test)]
mod tests;
