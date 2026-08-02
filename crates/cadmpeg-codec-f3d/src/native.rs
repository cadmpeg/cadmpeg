// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Autodesk Fusion native design and construction-history records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase, VersionContract};

use crate::history_records::AsmHistory;
use crate::records::{
    ActEntity, ActGuid, ActRootComponent, BodyNativeKey, BodyVisibility, ConstructionRecipe,
    CreationTimestamp, DesignBodyBinding, DesignBodyBounds, DesignBodyMember,
    DesignBodyRecipeOperand, DesignConfiguration, DesignConstructionOperandGroup,
    DesignConstructionOperandIdentity, DesignDimensionAnnotationFrame, DesignDimensionLocusGroup,
    DesignDimensionLocusPair, DesignDimensionNullLocusPair, DesignDimensionRecipeRecord,
    DesignEdgeIdentityOperand, DesignEdgeOperand, DesignEntityHeader, DesignEntitySelectionOperand,
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember, DesignFaceOperand,
    DesignFilletRadiusGroup, DesignMaterialAssignment, DesignObject, DesignParameter,
    DesignParameterCompanion, DesignParameterOwner, DesignParameterScope, DesignRecordHeader,
    DesignSketchPlacement, EdgeContinuity, EdgeOwnership, FaceSidedness, LostEdgeReference,
    MeshSurfaceSentinel, PersistentDesignLink, PersistentReference, PersistentSubentityTag,
    SketchCurveIdentity, SketchCurveLink, SketchPoint, SketchRelation, SketchSurface, SketchText,
    TolerantCoedgeParameters, TolerantEdgeTail, TolerantVertexTail, TransformHints,
    VertexOwnership, WireTopology, XrefDesign, XrefReference,
};

/// Current schema version for the Autodesk Fusion native namespace.
pub const F3D_NATIVE_VERSION: u32 = 5;

pub(crate) const F3D_ARENA_NAMES: &[&str] = &[
    "act_entities",
    "act_guids",
    "act_root_components",
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
    "design_configurations",
    "design_construction_operand_groups",
    "design_construction_operand_identities",
    "design_dimension_annotation_frames",
    "design_dimension_locus_groups",
    "design_dimension_locus_pairs",
    "design_dimension_null_locus_pairs",
    "design_dimension_recipe_records",
    "design_edge_identity_operands",
    "design_edge_operands",
    "design_entity_headers",
    "design_entity_selection_operands",
    "design_extrude_selection_groups",
    "design_extrude_selection_members",
    "design_face_operands",
    "design_fillet_radius_groups",
    "design_material_assignments",
    "design_objects",
    "design_parameter_companions",
    "design_parameter_owners",
    "design_parameter_scopes",
    "design_parameters",
    "design_record_headers",
    "design_sketch_placements",
    "edge_continuities",
    "edge_ownerships",
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

fn emit_asm_histories(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let records = model
        .asm_histories
        .iter()
        .cloned()
        .map(|mut history| {
            history.states.clear();
            history
        })
        .collect::<Vec<_>>();
    namespace.set_arena(row.arena, &records)
}

fn emit_asm_delta_states(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let records = model
        .asm_histories
        .iter()
        .flat_map(|history| history.states.iter().cloned())
        .map(|mut state| {
            state.bulletin_boards.clear();
            state.records.clear();
            state
        })
        .collect::<Vec<_>>();
    namespace.set_arena(row.arena, &records)
}

fn emit_asm_bulletin_boards(
    model: &F3dNative,
    row: &F3dFamilyRow,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let records = model
        .asm_histories
        .iter()
        .flat_map(|history| &history.states)
        .flat_map(|state| state.bulletin_boards.iter().cloned())
        .map(|mut board| {
            board.changes.clear();
            board
        })
        .collect::<Vec<_>>();
    namespace.set_arena(row.arena, &records)
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
        .flat_map(|board| board.changes.iter().cloned())
        .collect::<Vec<_>>();
    namespace.set_arena(row.arena, &records)
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
        .flat_map(|state| state.records.iter().cloned())
        .collect::<Vec<_>>();
    namespace.set_arena(row.arena, &records)
}

pub(crate) const F3D_FAMILIES: &[F3dFamilyRow] = &[
    F3dFamilyRow {
        arena: "act_entities",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_entities),
        len: |model| model.act_entities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_guids",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_guids),
        len: |model| model.act_guids.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "act_root_components",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.act_root_components),
        len: |model| model.act_root_components.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "body_native_keys",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.body_native_keys),
        len: |model| model.body_native_keys.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "body_visibilities",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.body_visibilities),
        len: |model| model.body_visibilities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_objects",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_objects),
        len: |model| model.design_objects.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_recipe_operands",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_body_recipe_operands)
        },
        len: |model| model.design_body_recipe_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_annotation_frames",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_annotation_frames)
        },
        len: |model| model.design_dimension_annotation_frames.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_locus_groups",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_locus_groups)
        },
        len: |model| model.design_dimension_locus_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_locus_pairs",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_locus_pairs)
        },
        len: |model| model.design_dimension_locus_pairs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_null_locus_pairs",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_null_locus_pairs)
        },
        len: |model| model.design_dimension_null_locus_pairs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_dimension_recipe_records",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_dimension_recipe_records)
        },
        len: |model| model.design_dimension_recipe_records.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_operands",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_edge_operands),
        len: |model| model.design_edge_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_edge_identity_operands",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_edge_identity_operands)
        },
        len: |model| model.design_edge_identity_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_entity_selection_operands",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_entity_selection_operands)
        },
        len: |model| model.design_entity_selection_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_face_operands",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_face_operands),
        len: |model| model.design_face_operands.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_construction_operand_groups",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_construction_operand_groups)
        },
        len: |model| model.design_construction_operand_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_construction_operand_identities",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_construction_operand_identities)
        },
        len: |model| model.design_construction_operand_identities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_extrude_selection_groups",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_extrude_selection_groups)
        },
        len: |model| model.design_extrude_selection_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_extrude_selection_members",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_extrude_selection_members)
        },
        len: |model| model.design_extrude_selection_members.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_fillet_radius_groups",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_fillet_radius_groups)
        },
        len: |model| model.design_fillet_radius_groups.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_companions",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_parameter_companions)
        },
        len: |model| model.design_parameter_companions.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_owners",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_parameter_owners)
        },
        len: |model| model.design_parameter_owners.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameter_scopes",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_parameter_scopes)
        },
        len: |model| model.design_parameter_scopes.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_parameters",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_parameters),
        len: |model| model.design_parameters.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_entity_headers",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_entity_headers),
        len: |model| model.design_entity_headers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_record_headers",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_record_headers),
        len: |model| model.design_record_headers.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_sketch_placements",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_sketch_placements)
        },
        len: |model| model.design_sketch_placements.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_bindings",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_body_bindings),
        len: |model| model.design_body_bindings.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_bounds",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_body_bounds),
        len: |model| model.design_body_bounds.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_body_members",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_body_members),
        len: |model| model.design_body_members.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_configurations",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.design_configurations),
        len: |model| model.design_configurations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "design_material_assignments",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.design_material_assignments)
        },
        len: |model| model.design_material_assignments.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "edge_continuities",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.edge_continuities),
        len: |model| model.edge_continuities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "edge_ownerships",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.edge_ownerships),
        len: |model| model.edge_ownerships.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "face_sidedness",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.face_sidedness),
        len: |model| model.face_sidedness.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "construction_recipes",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.construction_recipes),
        len: |model| model.construction_recipes.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "creation_timestamps",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.creation_timestamps),
        len: |model| model.creation_timestamps.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_design_links",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.persistent_design_links)
        },
        len: |model| model.persistent_design_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_references",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.persistent_references),
        len: |model| model.persistent_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "persistent_subentity_tags",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.persistent_subentity_tags)
        },
        len: |model| model.persistent_subentity_tags.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_curve_links",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_curve_links),
        len: |model| model.sketch_curve_links.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_relations",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_relations),
        len: |model| model.sketch_relations.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_points",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_points),
        len: |model| model.sketch_points.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_curve_identities",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.sketch_curve_identities)
        },
        len: |model| model.sketch_curve_identities.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_surfaces",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_surfaces),
        len: |model| model.sketch_surfaces.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "sketch_texts",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.sketch_texts),
        len: |model| model.sketch_texts.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "lost_edge_references",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.lost_edge_references),
        len: |model| model.lost_edge_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "mesh_surface_sentinels",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.mesh_surface_sentinels),
        len: |model| model.mesh_surface_sentinels.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "vertex_ownerships",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.vertex_ownerships),
        len: |model| model.vertex_ownerships.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_coedge_parameters",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| {
            namespace.set_arena(row.arena, &model.tolerant_coedge_parameters)
        },
        len: |model| model.tolerant_coedge_parameters.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_edge_tails",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.tolerant_edge_tails),
        len: |model| model.tolerant_edge_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "tolerant_vertex_tails",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.tolerant_vertex_tails),
        len: |model| model.tolerant_vertex_tails.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "transform_hints",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.transform_hints),
        len: |model| model.transform_hints.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "wire_topologies",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.wire_topologies),
        len: |model| model.wire_topologies.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "xref_designs",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.xref_designs),
        len: |model| model.xref_designs.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "xref_references",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: |model, row, namespace| namespace.set_arena(row.arena, &model.xref_references),
        len: |model| model.xref_references.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_histories",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
        emit: emit_asm_histories,
        len: |model| model.asm_histories.len(),
        counts_toward_emptiness: true,
    },
    F3dFamilyRow {
        arena: "asm_delta_states",
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
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
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
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
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
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
        tag: None,
        exactness: (),
        phase: Phase::ArenaOnly,
        note: None,
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
    Catalogue::new(
        F3D_FAMILIES,
        VersionContract {
            minimum: 0,
            maximum: u32::MAX,
        },
    );

/// Autodesk Fusion records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct F3dNative {
    /// Schema version this namespace was written under; see [`F3D_NATIVE_VERSION`].
    pub version: u32,
    /// Fusion ACT change-tracking table entities.
    #[serde(default)]
    pub act_entities: Vec<ActEntity>,
    /// Fusion ACT stream-wide asset/change-version GUID pool.
    #[serde(default)]
    pub act_guids: Vec<ActGuid>,
    /// Fusion ACT document-root-to-registry links.
    #[serde(default)]
    pub act_root_components: Vec<ActRootComponent>,
    /// Native Design-join keys stored on ASM bodies.
    #[serde(default)]
    pub body_native_keys: Vec<BodyNativeKey>,
    /// Design browser-node visibility joined to solved ASM bodies.
    #[serde(default)]
    pub body_visibilities: Vec<BodyVisibility>,
    /// Design `MetaStream` object-table records.
    #[serde(default)]
    pub design_objects: Vec<DesignObject>,
    /// Whole-body operands joined to persistent body construction recipes.
    #[serde(default)]
    pub design_body_recipe_operands: Vec<DesignBodyRecipeOperand>,
    /// Annotated paired dimension frames governing parameter companions.
    #[serde(default)]
    pub design_dimension_annotation_frames: Vec<DesignDimensionAnnotationFrame>,
    /// Typed paired loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_locus_pairs: Vec<DesignDimensionLocusPair>,
    /// Counted typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_locus_groups: Vec<DesignDimensionLocusGroup>,
    /// Null-plus-typed loci recovered from dimensional companion graphs.
    #[serde(default)]
    pub design_dimension_null_locus_pairs: Vec<DesignDimensionNullLocusPair>,
    /// Indexed records containing dimension-owned construction recipes.
    #[serde(default)]
    pub design_dimension_recipe_records: Vec<DesignDimensionRecipeRecord>,
    /// Edge-selection operands recovered from Fillet and Chamfer scopes.
    #[serde(default)]
    pub design_edge_operands: Vec<DesignEdgeOperand>,
    /// Persistent selection identities recovered from Fillet and Chamfer groups.
    #[serde(default)]
    pub design_edge_identity_operands: Vec<DesignEdgeIdentityOperand>,
    /// Face-selection operands recovered from Extrude construction groups.
    #[serde(default)]
    pub design_face_operands: Vec<DesignFaceOperand>,
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
    /// Fixed-width owner frames for indexed Design parameters.
    #[serde(default)]
    pub design_parameter_owners: Vec<DesignParameterOwner>,
    /// Sketch and construction-operation records that scope parameters.
    #[serde(default)]
    pub design_parameter_scopes: Vec<DesignParameterScope>,
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

impl Default for F3dNative {
    fn default() -> Self {
        Self {
            version: F3D_NATIVE_VERSION,
            act_entities: Vec::new(),
            act_guids: Vec::new(),
            act_root_components: Vec::new(),
            body_native_keys: Vec::new(),
            body_visibilities: Vec::new(),
            design_objects: Vec::new(),
            design_body_recipe_operands: Vec::new(),
            design_dimension_annotation_frames: Vec::new(),
            design_dimension_locus_pairs: Vec::new(),
            design_dimension_locus_groups: Vec::new(),
            design_dimension_null_locus_pairs: Vec::new(),
            design_dimension_recipe_records: Vec::new(),
            design_edge_operands: Vec::new(),
            design_edge_identity_operands: Vec::new(),
            design_face_operands: Vec::new(),
            design_construction_operand_groups: Vec::new(),
            design_construction_operand_identities: Vec::new(),
            design_extrude_selection_groups: Vec::new(),
            design_extrude_selection_members: Vec::new(),
            design_entity_selection_operands: Vec::new(),
            design_fillet_radius_groups: Vec::new(),
            design_parameter_companions: Vec::new(),
            design_parameter_owners: Vec::new(),
            design_parameter_scopes: Vec::new(),
            design_parameters: Vec::new(),
            design_sketch_placements: Vec::new(),
            design_entity_headers: Vec::new(),
            design_record_headers: Vec::new(),
            design_body_members: Vec::new(),
            design_body_bindings: Vec::new(),
            design_body_bounds: Vec::new(),
            design_configurations: Vec::new(),
            design_material_assignments: Vec::new(),
            edge_continuities: Vec::new(),
            edge_ownerships: Vec::new(),
            face_sidedness: Vec::new(),
            construction_recipes: Vec::new(),
            creation_timestamps: Vec::new(),
            persistent_design_links: Vec::new(),
            persistent_references: Vec::new(),
            persistent_subentity_tags: Vec::new(),
            sketch_curve_links: Vec::new(),
            sketch_relations: Vec::new(),
            sketch_points: Vec::new(),
            sketch_curve_identities: Vec::new(),
            sketch_surfaces: Vec::new(),
            sketch_texts: Vec::new(),
            lost_edge_references: Vec::new(),
            mesh_surface_sentinels: Vec::new(),
            vertex_ownerships: Vec::new(),
            tolerant_coedge_parameters: Vec::new(),
            tolerant_edge_tails: Vec::new(),
            tolerant_vertex_tails: Vec::new(),
            transform_hints: Vec::new(),
            wire_topologies: Vec::new(),
            xref_designs: Vec::new(),
            xref_references: Vec::new(),
            asm_histories: Vec::new(),
        }
    }
}

impl F3dNative {
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let mut native = Self {
            version: namespace.version,
            act_entities: namespace.arena_as("act_entities")?,
            act_guids: namespace.arena_as("act_guids")?,
            act_root_components: namespace.arena_as("act_root_components")?,
            body_native_keys: namespace.arena_as("body_native_keys")?,
            body_visibilities: namespace.arena_as("body_visibilities")?,
            design_objects: namespace.arena_as("design_objects")?,
            design_body_recipe_operands: namespace.arena_as("design_body_recipe_operands")?,
            design_dimension_annotation_frames: namespace
                .arena_as("design_dimension_annotation_frames")?,
            design_dimension_locus_groups: namespace.arena_as("design_dimension_locus_groups")?,
            design_dimension_locus_pairs: namespace.arena_as("design_dimension_locus_pairs")?,
            design_dimension_null_locus_pairs: namespace
                .arena_as("design_dimension_null_locus_pairs")?,
            design_dimension_recipe_records: namespace
                .arena_as("design_dimension_recipe_records")?,
            design_edge_operands: namespace.arena_as("design_edge_operands")?,
            design_edge_identity_operands: namespace.arena_as("design_edge_identity_operands")?,
            design_entity_selection_operands: namespace
                .arena_as("design_entity_selection_operands")?,
            design_face_operands: namespace.arena_as("design_face_operands")?,
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
        let mut states: Vec<crate::history_records::AsmDeltaState> =
            namespace.arena_as("asm_delta_states")?;
        let mut boards: Vec<crate::history_records::AsmBulletinBoard> =
            namespace.arena_as("asm_bulletin_boards")?;
        let changes: Vec<crate::history_records::AsmEntityChange> =
            namespace.arena_as("asm_entity_changes")?;
        let records: Vec<crate::history_records::AsmHistoryRecord> =
            namespace.arena_as("asm_history_records")?;
        for board in &mut boards {
            board.changes = changes
                .iter()
                .filter(|change| change.parent == board.id)
                .cloned()
                .collect();
        }
        for state in &mut states {
            state.bulletin_boards = boards
                .iter()
                .filter(|board| board.parent == state.id)
                .cloned()
                .collect();
            state.records = records
                .iter()
                .filter(|record| record.parent == state.id)
                .cloned()
                .collect();
        }
        for history in &mut native.asm_histories {
            history.states = states
                .iter()
                .filter(|state| state.parent == history.id)
                .cloned()
                .collect();
        }
        Ok(native)
    }

    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        namespace.version = F3D_NATIVE_VERSION;
        F3D_CATALOGUE.emit_all(self, namespace)?;
        debug_assert!(F3D_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }
}
