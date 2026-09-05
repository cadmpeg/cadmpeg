// SPDX-License-Identifier: Apache-2.0
//! Structural `AllFeatur` feature-to-generated-entity bindings.
//!
//! A mixed generated-entity table is `f8 <count> f7 <table-class> fb e3`, followed by
//! exactly `<count>` compact entity identifiers, each terminated by `e3`.
//! `f7 <entry-class>` may prefix the first entry. The table belongs to an `AllFeatur` row only
//! when its byte offset is bounded by that row's known feature-id header.

mod definitions;
mod entity;
mod helpers;
mod operations;
mod rows;

#[cfg(test)]
mod tests;

pub use definitions::equation_table;
pub(crate) use definitions::saved_entity_offset;
pub use definitions::{
    bind_definition_owners, bind_replay_definition_owners, bind_section_owners,
    bind_trimmed_definition_owners, definition_revolution_extents, definitions, depdb_definitions,
    depdb_section_definition, placement_instructions, positional_replay_definitions, BinaryFlag,
    DimensionUnit, FeatureBoundedCurveSegment, FeatureCenteredLineSegment, FeatureCircleSegment,
    FeatureDefinition, FeatureDimension, FeatureDimensionTable, FeatureOpaqueSegment,
    FeatureOrderTable, FeatureParameterFrameKind, FeaturePointSegment, FeatureReferenceLineSegment,
    FeatureRelation, FeatureRelationTable, FeatureRelationTriple, FeatureSavedArc,
    FeatureSavedEntity, FeatureSavedLine, FeatureSavedSpline, FeatureSection3d, FeatureSegment,
    FeatureSegmentKind, FeatureSegmentTable, FeatureSkamp, FeatureSkampItem,
    FeatureSolverTableHeader, FeatureTrimEntity, FeatureVariableRow, OutlinePhase, TrimEntityKind,
};
pub(crate) use definitions::{FeatureEquation, FeatureVariableTable};
#[cfg(test)]
pub use definitions::{
    FeatureOrderRow, FeatureParameterFrame, FeatureSavedCircle, FeatureSavedConic,
    FeatureSavedSection, FeatureSectionOrientation, FeatureSectionPoint,
    FeatureSectionReferencePlane, FeatureTrimBucket, FeatureTrimEntityTable, FeatureTrimVertex,
    FeatureTrimVertexTable,
};
#[cfg(test)]
pub(crate) use entity::dummy_table_entry;
pub use entity::{
    entity_graph, entity_tables, FeatureEntity, FeatureEntityReference, FeatureEntityTable,
    FeatureEntityTableEntry,
};
#[allow(unused_imports)]
pub use operations::{
    operation_states, operations, reference_names, DepdbPrefix, FeatureOperation, FeatureRecipe,
    FeatureRecipeEffect, FeatureRecipeKind, FeatureReferenceName, IdKeyword, OperationKind,
    OperationName,
};
pub use rows::{
    affected_ids, choice_fields, choices, geometry_tables, loop_history_entries,
    loop_restore_directions, replay_affected_ids, revolution_extents, rows,
    surface_merge_replay_affected_ids, AffectedIdKind, FeatureAffectedIds, FeatureChoice,
    FeatureChoiceField, FeatureFieldValue, FeatureGeometryTable, FeatureGeometryTableKind,
    FeatureLoopHistoryBoundary, FeatureLoopHistoryEntry, FeatureLoopRestoreDirection,
    FeatureReplayAffectedIds, FeatureRevolutionExtent, FeatureRevolutionExtentKind, FeatureRow,
    FeatureSurfaceMergeAffectedIds, LoopRestoreDirectionLane, ReplayExtentSource,
};
pub(crate) use rows::{round_replay_scalars, FeatureRoundReplayScalar};
