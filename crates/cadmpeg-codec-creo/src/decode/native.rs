// SPDX-License-Identifier: Apache-2.0
//! Native-arena emission layer for the `creo` namespace.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use serde::Serialize;

/// Closed arena domain of the Creo native namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreoArena {
    ExpandedSections,
    LegacyIntegerValues,
    LegacyObjects,
    LegacyRealValues,
    LegacyStringValues,
    LegacyType3Values,
    LegacyType4Values,
    LegacyType5Values,
    LegacyType6Values,
    LegacyType7Values,
    LegacyType9Values,
    LegacyType11Values,
    DoubleXarTables,
    PrimitiveScalarArrays,
    ReferenceLines,
    ReferenceCircles,
    ReferenceConics,
    ReferenceEllipses,
    SurfaceRows,
    NonvisibleSurfaceRows,
    CrossSectionSurfaceRows,
    SurfaceContours,
    NonvisibleSurfaceContours,
    CrossSectionSurfaceContours,
    SurfacePrototypes,
    NonvisibleSurfacePrototypes,
    TabulatedCylinderCurveReplays,
    CurveParameters,
    NonvisibleCurveParameters,
    FcCurveCoordinates,
    Fc05Circles,
    Fc05CylinderCapPairs,
    PrototypePcurves,
    CurvePrototypeTopology,
    CurvePrototypes,
    NonvisibleCurvePrototypes,
    CrossSectionCurvePrototypes,
    CurveTopologyRows,
    NonvisibleCurveTopologyRows,
    CrossSectionCurveRows,
    LoopArrayFrames,
    LoopArrayRecords,
    HalfEdges,
    Loops,
    TopologicalVertices,
    HalfEdgeVertexIncidence,
    FaceComponents,
    BrepFaceAdmissionRejections,
    SurfaceParameters,
    NonvisibleSurfaceParameters,
    CrossSectionSurfaceParameters,
    PlaneLocalSystems,
    CrossSectionPlaneLocalSystems,
    PlaneEnvelopes,
    CrossSectionPlaneEnvelopes,
    OutlinePlanes,
    PositionalFramePlanes,
    CrossSectionOutlinePlanes,
    DatumPlanes,
    DatumCylinders,
    FeatureSectionTransforms,
    FeaturePlacementInstructions,
    PcurveEndpoints,
    FeatureDefinitions,
    FeatureEntities,
    FeatureEntityReferences,
    FeatureEntityTables,
    FeatureSurfaceReplays,
    FeatureGeometryTables,
    FeatureLoopHistoryEntries,
    FeatureAffectedIds,
    FeatureReplayAffectedIds,
    SurfaceMergeReplayAffectedIds,
    FeatureLoopRestoreDirections,
    FeatureRevolutionExtents,
    FeatureRows,
    DepdbRecipeRows,
    FeatureChoices,
    FeatureChoiceFields,
    Sketches,
    CurveExpressions,
    FeatureOperationStates,
    FeatureReferenceNames,
    Configuration,
    ConfigurationDriverTables,
}

impl CreoArena {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ExpandedSections => "expanded_sections",
            Self::LegacyIntegerValues => "legacy_integer_values",
            Self::LegacyObjects => "legacy_objects",
            Self::LegacyRealValues => "legacy_real_values",
            Self::LegacyStringValues => "legacy_string_values",
            Self::LegacyType3Values => "legacy_type_3_values",
            Self::LegacyType4Values => "legacy_type_4_values",
            Self::LegacyType5Values => "legacy_type_5_values",
            Self::LegacyType6Values => "legacy_type_6_values",
            Self::LegacyType7Values => "legacy_type_7_values",
            Self::LegacyType9Values => "legacy_type_9_values",
            Self::LegacyType11Values => "legacy_type_11_values",
            Self::DoubleXarTables => "double_xar_tables",
            Self::PrimitiveScalarArrays => "primitive_scalar_arrays",
            Self::ReferenceLines => "reference_lines",
            Self::ReferenceCircles => "reference_circles",
            Self::ReferenceConics => "reference_conics",
            Self::ReferenceEllipses => "reference_ellipses",
            Self::SurfaceRows => "surface_rows",
            Self::NonvisibleSurfaceRows => "nonvisible_surface_rows",
            Self::CrossSectionSurfaceRows => "cross_section_surface_rows",
            Self::SurfaceContours => "surface_contours",
            Self::NonvisibleSurfaceContours => "nonvisible_surface_contours",
            Self::CrossSectionSurfaceContours => "cross_section_surface_contours",
            Self::SurfacePrototypes => "surface_prototypes",
            Self::NonvisibleSurfacePrototypes => "nonvisible_surface_prototypes",
            Self::TabulatedCylinderCurveReplays => "tabulated_cylinder_curve_replays",
            Self::CurveParameters => "curve_parameters",
            Self::NonvisibleCurveParameters => "nonvisible_curve_parameters",
            Self::FcCurveCoordinates => "fc_curve_coordinates",
            Self::Fc05Circles => "fc05_circles",
            Self::Fc05CylinderCapPairs => "fc05_cylinder_cap_pairs",
            Self::PrototypePcurves => "prototype_pcurves",
            Self::CurvePrototypeTopology => "curve_prototype_topology",
            Self::CurvePrototypes => "curve_prototypes",
            Self::NonvisibleCurvePrototypes => "nonvisible_curve_prototypes",
            Self::CrossSectionCurvePrototypes => "cross_section_curve_prototypes",
            Self::CurveTopologyRows => "curve_topology_rows",
            Self::NonvisibleCurveTopologyRows => "nonvisible_curve_topology_rows",
            Self::CrossSectionCurveRows => "cross_section_curve_rows",
            Self::LoopArrayFrames => "loop_array_frames",
            Self::LoopArrayRecords => "loop_array_records",
            Self::HalfEdges => "half_edges",
            Self::Loops => "loops",
            Self::TopologicalVertices => "topological_vertices",
            Self::HalfEdgeVertexIncidence => "half_edge_vertex_incidence",
            Self::FaceComponents => "face_components",
            Self::BrepFaceAdmissionRejections => "brep_face_admission_rejections",
            Self::SurfaceParameters => "surface_parameters",
            Self::NonvisibleSurfaceParameters => "nonvisible_surface_parameters",
            Self::CrossSectionSurfaceParameters => "cross_section_surface_parameters",
            Self::PlaneLocalSystems => "plane_local_systems",
            Self::CrossSectionPlaneLocalSystems => "cross_section_plane_local_systems",
            Self::PlaneEnvelopes => "plane_envelopes",
            Self::CrossSectionPlaneEnvelopes => "cross_section_plane_envelopes",
            Self::OutlinePlanes => "outline_planes",
            Self::PositionalFramePlanes => "positional_frame_planes",
            Self::CrossSectionOutlinePlanes => "cross_section_outline_planes",
            Self::DatumPlanes => "datum_planes",
            Self::DatumCylinders => "datum_cylinders",
            Self::FeatureSectionTransforms => "feature_section_transforms",
            Self::FeaturePlacementInstructions => "feature_placement_instructions",
            Self::PcurveEndpoints => "pcurve_endpoints",
            Self::FeatureDefinitions => "feature_definitions",
            Self::FeatureEntities => "feature_entities",
            Self::FeatureEntityReferences => "feature_entity_references",
            Self::FeatureEntityTables => "feature_entity_tables",
            Self::FeatureSurfaceReplays => "feature_surface_replays",
            Self::FeatureGeometryTables => "feature_geometry_tables",
            Self::FeatureLoopHistoryEntries => "feature_loop_history_entries",
            Self::FeatureAffectedIds => "feature_affected_ids",
            Self::FeatureReplayAffectedIds => "feature_replay_affected_ids",
            Self::SurfaceMergeReplayAffectedIds => "surface_merge_replay_affected_ids",
            Self::FeatureLoopRestoreDirections => "feature_loop_restore_directions",
            Self::FeatureRevolutionExtents => "feature_revolution_extents",
            Self::FeatureRows => "feature_rows",
            Self::DepdbRecipeRows => "depdb_recipe_rows",
            Self::FeatureChoices => "feature_choices",
            Self::FeatureChoiceFields => "feature_choice_fields",
            Self::Sketches => "sketches",
            Self::CurveExpressions => "curve_expressions",
            Self::FeatureOperationStates => "feature_operation_states",
            Self::FeatureReferenceNames => "feature_reference_names",
            Self::Configuration => "configuration",
            Self::ConfigurationDriverTables => "configuration_driver_tables",
        }
    }
}

/// Record one native-source provenance annotation.
///
/// Names the source stream `creo:{source_stream}`, tags the note at `offset`, and
/// records the transfer exactness. Shared by the model-transfer path and every
/// arena emission.
pub(super) fn annotate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    source_stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) -> Result<(), CodecError> {
    annotations.annotate_admitted(
        ctx,
        id,
        format_args!("creo:{source_stream}"),
        offset,
        tag,
        exactness,
    )
}

/// Store `records` as native arena `key`, skipping empty input.
///
/// An empty slice returns without touching the namespace, so an arena that was
/// absent for empty input stays absent — flipping it to present-but-empty would
/// be an observable change. On non-empty input the records are serialized under
/// `key`.
pub(super) fn store_arena<T: Serialize>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    key: CreoArena,
    records: &[T],
) -> Result<(), CodecError> {
    if records.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut("creo");
    namespace.set_arena(ctx, key.as_str(), records)?;
    Ok(())
}

/// Annotate each record with `annotate_each`, then store them as arena `key`.
pub(super) fn emit_arena<T, F>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    key: CreoArena,
    records: &[T],
    mut annotate_each: F,
) -> Result<(), CodecError>
where
    T: Serialize,
    F: FnMut(&mut AnnotationBuilder, &T) -> Result<(), CodecError>,
{
    for record in records {
        annotate_each(annotations, record)?;
    }
    store_arena(ctx, ir, key, records)
}

/// Records and provenance fields for one native arena.
pub(super) struct UniformArena<'a, T> {
    pub key: CreoArena,
    pub records: &'a [T],
    pub id: fn(&T) -> &str,
    pub stream: fn(&T) -> &str,
    pub offset: fn(&T) -> u64,
    pub tag: &'a str,
    pub exactness: Exactness,
}

/// Emit an arena whose provenance comes entirely from each record's own fields.
pub(super) fn emit_uniform<T: Serialize>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    arena: &UniformArena<'_, T>,
) -> Result<(), CodecError> {
    emit_arena(
        ctx,
        ir,
        annotations,
        arena.key,
        arena.records,
        |annotations, record| {
            annotate(
                ctx,
                annotations,
                (arena.id)(record),
                (arena.stream)(record),
                (arena.offset)(record),
                arena.tag,
                arena.exactness,
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::CreoArena;

    #[test]
    fn closed_arena_variants_preserve_unique_wire_names() {
        let arenas = [
            (CreoArena::ExpandedSections, "expanded_sections"),
            (CreoArena::LegacyIntegerValues, "legacy_integer_values"),
            (CreoArena::LegacyObjects, "legacy_objects"),
            (CreoArena::LegacyRealValues, "legacy_real_values"),
            (CreoArena::LegacyStringValues, "legacy_string_values"),
            (CreoArena::LegacyType3Values, "legacy_type_3_values"),
            (CreoArena::LegacyType4Values, "legacy_type_4_values"),
            (CreoArena::LegacyType5Values, "legacy_type_5_values"),
            (CreoArena::LegacyType6Values, "legacy_type_6_values"),
            (CreoArena::LegacyType7Values, "legacy_type_7_values"),
            (CreoArena::LegacyType9Values, "legacy_type_9_values"),
            (CreoArena::LegacyType11Values, "legacy_type_11_values"),
            (CreoArena::DoubleXarTables, "double_xar_tables"),
            (CreoArena::PrimitiveScalarArrays, "primitive_scalar_arrays"),
            (CreoArena::ReferenceLines, "reference_lines"),
            (CreoArena::ReferenceCircles, "reference_circles"),
            (CreoArena::ReferenceConics, "reference_conics"),
            (CreoArena::ReferenceEllipses, "reference_ellipses"),
            (CreoArena::SurfaceRows, "surface_rows"),
            (CreoArena::NonvisibleSurfaceRows, "nonvisible_surface_rows"),
            (CreoArena::CrossSectionSurfaceRows, "cross_section_surface_rows"),
            (CreoArena::SurfaceContours, "surface_contours"),
            (CreoArena::NonvisibleSurfaceContours, "nonvisible_surface_contours"),
            (CreoArena::CrossSectionSurfaceContours, "cross_section_surface_contours"),
            (CreoArena::SurfacePrototypes, "surface_prototypes"),
            (CreoArena::NonvisibleSurfacePrototypes, "nonvisible_surface_prototypes"),
            (CreoArena::TabulatedCylinderCurveReplays, "tabulated_cylinder_curve_replays"),
            (CreoArena::CurveParameters, "curve_parameters"),
            (CreoArena::NonvisibleCurveParameters, "nonvisible_curve_parameters"),
            (CreoArena::FcCurveCoordinates, "fc_curve_coordinates"),
            (CreoArena::Fc05Circles, "fc05_circles"),
            (CreoArena::Fc05CylinderCapPairs, "fc05_cylinder_cap_pairs"),
            (CreoArena::PrototypePcurves, "prototype_pcurves"),
            (CreoArena::CurvePrototypeTopology, "curve_prototype_topology"),
            (CreoArena::CurvePrototypes, "curve_prototypes"),
            (CreoArena::NonvisibleCurvePrototypes, "nonvisible_curve_prototypes"),
            (CreoArena::CrossSectionCurvePrototypes, "cross_section_curve_prototypes"),
            (CreoArena::CurveTopologyRows, "curve_topology_rows"),
            (CreoArena::NonvisibleCurveTopologyRows, "nonvisible_curve_topology_rows"),
            (CreoArena::CrossSectionCurveRows, "cross_section_curve_rows"),
            (CreoArena::LoopArrayFrames, "loop_array_frames"),
            (CreoArena::LoopArrayRecords, "loop_array_records"),
            (CreoArena::HalfEdges, "half_edges"),
            (CreoArena::Loops, "loops"),
            (CreoArena::TopologicalVertices, "topological_vertices"),
            (CreoArena::HalfEdgeVertexIncidence, "half_edge_vertex_incidence"),
            (CreoArena::FaceComponents, "face_components"),
            (CreoArena::BrepFaceAdmissionRejections, "brep_face_admission_rejections"),
            (CreoArena::SurfaceParameters, "surface_parameters"),
            (CreoArena::NonvisibleSurfaceParameters, "nonvisible_surface_parameters"),
            (CreoArena::CrossSectionSurfaceParameters, "cross_section_surface_parameters"),
            (CreoArena::PlaneLocalSystems, "plane_local_systems"),
            (CreoArena::CrossSectionPlaneLocalSystems, "cross_section_plane_local_systems"),
            (CreoArena::PlaneEnvelopes, "plane_envelopes"),
            (CreoArena::CrossSectionPlaneEnvelopes, "cross_section_plane_envelopes"),
            (CreoArena::OutlinePlanes, "outline_planes"),
            (CreoArena::PositionalFramePlanes, "positional_frame_planes"),
            (CreoArena::CrossSectionOutlinePlanes, "cross_section_outline_planes"),
            (CreoArena::DatumPlanes, "datum_planes"),
            (CreoArena::DatumCylinders, "datum_cylinders"),
            (CreoArena::FeatureSectionTransforms, "feature_section_transforms"),
            (CreoArena::FeaturePlacementInstructions, "feature_placement_instructions"),
            (CreoArena::PcurveEndpoints, "pcurve_endpoints"),
            (CreoArena::FeatureDefinitions, "feature_definitions"),
            (CreoArena::FeatureEntities, "feature_entities"),
            (CreoArena::FeatureEntityReferences, "feature_entity_references"),
            (CreoArena::FeatureEntityTables, "feature_entity_tables"),
            (CreoArena::FeatureSurfaceReplays, "feature_surface_replays"),
            (CreoArena::FeatureGeometryTables, "feature_geometry_tables"),
            (CreoArena::FeatureLoopHistoryEntries, "feature_loop_history_entries"),
            (CreoArena::FeatureAffectedIds, "feature_affected_ids"),
            (CreoArena::FeatureReplayAffectedIds, "feature_replay_affected_ids"),
            (CreoArena::SurfaceMergeReplayAffectedIds, "surface_merge_replay_affected_ids"),
            (CreoArena::FeatureLoopRestoreDirections, "feature_loop_restore_directions"),
            (CreoArena::FeatureRevolutionExtents, "feature_revolution_extents"),
            (CreoArena::FeatureRows, "feature_rows"),
            (CreoArena::DepdbRecipeRows, "depdb_recipe_rows"),
            (CreoArena::FeatureChoices, "feature_choices"),
            (CreoArena::FeatureChoiceFields, "feature_choice_fields"),
            (CreoArena::Sketches, "sketches"),
            (CreoArena::CurveExpressions, "curve_expressions"),
            (CreoArena::FeatureOperationStates, "feature_operation_states"),
            (CreoArena::FeatureReferenceNames, "feature_reference_names"),
            (CreoArena::Configuration, "configuration"),
            (CreoArena::ConfigurationDriverTables, "configuration_driver_tables"),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (arena, expected) in arenas {
            assert_eq!(arena.as_str(), expected);
            assert!(seen.insert(arena.as_str()), "duplicate arena spelling");
        }
    }
}
