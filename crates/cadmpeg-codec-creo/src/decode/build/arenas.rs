// SPDX-License-Identifier: Apache-2.0
//! Native reference-geometry and model-namespace arena emission.

use crate::decode::native::CreoArena;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;

use super::super::coverage::source_section_ref;
use super::super::expanded::{
    fc05_circle_records, fc05_cylinder_cap_pair_records, feature_surface_replay_associations,
};
use super::super::native::emit_arena;
use super::super::native::{annotate, emit_uniform, store_arena, UniformArena};
use super::super::records::family_table_record;
use super::super::records::{
    cross_section_curve_row_records, curve_expression_records, curve_parameter_records,
    curve_prototype_records, curve_prototype_topology_records, curve_topology_row_records,
    datum_cylinder_records, datum_plane_records, depdb_recipe_row_records, face_component_records,
    fc_curve_coordinate_records, feature_affected_id_records, feature_choice_field_records,
    feature_choice_records, feature_definition_records, feature_entity_records,
    feature_entity_reference_records, feature_entity_table_records, feature_geometry_table_records,
    feature_loop_history_entry_records, feature_loop_restore_direction_records,
    feature_operation_state_records, feature_placement_instruction_records,
    feature_reference_name_records, feature_replay_affected_id_records,
    feature_revolution_extent_records, feature_row_records, feature_section_transform_records,
    half_edge_records, half_edge_vertex_incidence_records, loop_array_frame_records,
    loop_array_record_records, loop_records, outline_plane_records, pcurve_endpoint_records,
    plane_envelope_records, plane_local_system_records, prototype_pcurve_records,
    reference_circle_records, reference_conic_records, reference_ellipse_records,
    reference_line_records, sketch_records, surface_contour_records,
    surface_merge_replay_affected_id_records, surface_parameter_records, surface_prototype_records,
    surface_row_records, tabulated_cylinder_curve_replay_records, topological_vertex_records,
};
use super::super::surfaces::brep::BrepTransferDiagnostics;

/// Emit the `MdlRefInfo` reference-geometry arenas.
///
/// Reference lines, circles, conics, and ellipse carriers, each annotated
/// against the `MdlRefInfo` stream at the record offset.
pub(super) fn emit_reference_arenas(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::ReferenceLines,
            records: &reference_line_records(ctx, scan)?,
            id: |record| &record.id,
            stream: |_| "MdlRefInfo",
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "reference_line_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::ReferenceCircles,
            records: &reference_circle_records(ctx, scan)?,
            id: |record| &record.id,
            stream: |_| "MdlRefInfo",
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "reference_circle_record",
            exactness: Exactness::Derived,
        },
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::ReferenceConics,
            records: &reference_conic_records(ctx, scan)?,
            id: |record| &record.id,
            stream: |_| "MdlRefInfo",
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "reference_conic_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::ReferenceEllipses,
            records: &reference_ellipse_records(ctx, scan)?,
            id: |record| &record.id,
            stream: |_| "MdlRefInfo",
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "reference_ellipse_carrier",
            exactness: Exactness::Derived,
        },
    )?;
    Ok(())
}

/// Emit the surface, curve, topology, plane, and feature arenas.
///
/// Each arena is built from the scan and stored under its native key in the
/// order the source streams are read; that order fixes the annotation stream
/// numbering, so the emissions must not be reordered.
pub(super) fn emit_geometry_arenas(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    brep_diagnostics: &BrepTransferDiagnostics,
) -> Result<(), CodecError> {
    let surface_rows = surface_row_records(ctx, scan, &scan.surfaces.rows, "visibgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::SurfaceRows,
            records: &surface_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "surface_namespace_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_surface_rows =
        surface_row_records(ctx, scan, &scan.surfaces.nonvisible_rows, "novisgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleSurfaceRows,
            records: &nonvisible_surface_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "nonvisible_surface_namespace_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let cross_section_surface_rows = surface_row_records(
        ctx,
        scan,
        &scan.surfaces.cross_section_rows,
        "cross_section_geometry",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CrossSectionSurfaceRows,
            records: &cross_section_surface_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "cross_section_surface_namespace_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let surface_contours =
        surface_contour_records(ctx, scan, &scan.surfaces.contours, "visibgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::SurfaceContours,
            records: &surface_contours,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "surface_contour_chain_entry",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_surface_contours =
        surface_contour_records(ctx, scan, &scan.surfaces.nonvisible_contours, "novisgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleSurfaceContours,
            records: &nonvisible_surface_contours,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "nonvisible_surface_contour_chain_entry",
            exactness: Exactness::ByteExact,
        },
    )?;
    let cross_section_surface_contours = surface_contour_records(
        ctx,
        scan,
        &scan.surfaces.cross_section_contours,
        "cross_section_geometry",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CrossSectionSurfaceContours,
            records: &cross_section_surface_contours,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "cross_section_surface_contour_chain_entry",
            exactness: Exactness::ByteExact,
        },
    )?;
    let surface_prototypes =
        surface_prototype_records(ctx, scan, &scan.surfaces.prototype_records, "visibgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::SurfacePrototypes,
            records: &surface_prototypes,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "surface_prototype_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_surface_prototypes = surface_prototype_records(
        ctx,
        scan,
        &scan.surfaces.nonvisible_prototype_records,
        "novisgeom",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleSurfacePrototypes,
            records: &nonvisible_surface_prototypes,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "nonvisible_surface_prototype_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    let tabulated_cylinder_curve_replays = tabulated_cylinder_curve_replay_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::TabulatedCylinderCurveReplays,
            records: &tabulated_cylinder_curve_replays,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "tabulated_cylinder_curve_replay",
            exactness: Exactness::ByteExact,
        },
    )?;
    let curve_parameters =
        curve_parameter_records(ctx, scan, &scan.curves.parameters, "visibgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CurveParameters,
            records: &curve_parameters,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "curve_parameter_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_curve_parameters =
        curve_parameter_records(ctx, scan, &scan.curves.nonvisible_parameters, "novisgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleCurveParameters,
            records: &nonvisible_curve_parameters,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "nonvisible_curve_parameter_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    let fc_curve_coordinates = fc_curve_coordinate_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FcCurveCoordinates,
            records: &fc_curve_coordinates,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "fc_curve_coordinates",
            exactness: Exactness::ByteExact,
        },
    )?;
    let fc05_circles = fc05_circle_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::Fc05Circles, &fc05_circles)?;
    let fc05_cylinder_cap_pairs = fc05_cylinder_cap_pair_records(ctx, scan)?;
    store_arena(
        ctx,
        ir,
        CreoArena::Fc05CylinderCapPairs,
        &fc05_cylinder_cap_pairs,
    )?;
    let prototype_pcurves = prototype_pcurve_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::PrototypePcurves, &prototype_pcurves)?;
    let curve_prototype_topology = curve_prototype_topology_records(ctx, scan)?;
    store_arena(
        ctx,
        ir,
        CreoArena::CurvePrototypeTopology,
        &curve_prototype_topology,
    )?;
    let curve_prototypes =
        curve_prototype_records(ctx, scan, &scan.curves.prototypes, "creo:curve:prototype")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CurvePrototypes,
            records: &curve_prototypes,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "curve_prototype",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_curve_prototypes = curve_prototype_records(
        ctx,
        scan,
        &scan.curves.nonvisible_prototypes,
        "creo:novisgeom:curve_prototype",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleCurvePrototypes,
            records: &nonvisible_curve_prototypes,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "nonvisible_curve_prototype",
            exactness: Exactness::ByteExact,
        },
    )?;
    let cross_section_curve_prototypes = curve_prototype_records(
        ctx,
        scan,
        &scan.curves.cross_section_prototypes,
        "creo:cross_section_geometry:curve_prototype",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CrossSectionCurvePrototypes,
            records: &cross_section_curve_prototypes,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "cross_section_curve_prototype",
            exactness: Exactness::ByteExact,
        },
    )?;
    let curve_topology_rows =
        curve_topology_row_records(ctx, scan, &scan.curves.topology_rows, "visibgeom")?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CurveTopologyRows,
            records: &curve_topology_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "curve_topology_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_curve_topology_rows = curve_topology_row_records(
        ctx,
        scan,
        &scan.curves.nonvisible_topology_rows,
        "novisgeom",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleCurveTopologyRows,
            records: &nonvisible_curve_topology_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "nonvisible_curve_topology_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let cross_section_curve_rows = cross_section_curve_row_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CrossSectionCurveRows,
            records: &cross_section_curve_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "cross_section_curve_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let loop_array_frames = loop_array_frame_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::LoopArrayFrames, &loop_array_frames)?;
    let loop_array_records = loop_array_record_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::LoopArrayRecords,
            records: &loop_array_records,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "loop_array_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    let half_edges = half_edge_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::HalfEdges,
            records: &half_edges,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "native_half_edge",
            exactness: Exactness::Derived,
        },
    )?;
    let native_loops = loop_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::Loops, &native_loops)?;
    let topological_vertices = topological_vertex_records(ctx, scan)?;
    store_arena(
        ctx,
        ir,
        CreoArena::TopologicalVertices,
        &topological_vertices,
    )?;
    let half_edge_vertex_incidence = half_edge_vertex_incidence_records(ctx, scan)?;
    store_arena(
        ctx,
        ir,
        CreoArena::HalfEdgeVertexIncidence,
        &half_edge_vertex_incidence,
    )?;
    let face_components = face_component_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::FaceComponents, &face_components)?;
    let face_admission_rejections = brep_diagnostics.face_admission_rejection_records(ctx)?;
    store_arena(
        ctx,
        ir,
        CreoArena::BrepFaceAdmissionRejections,
        &face_admission_rejections,
    )?;
    let surface_parameters = surface_parameter_records(
        ctx,
        scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::SurfaceParameters,
            records: &surface_parameters,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.body_offset),
            tag: "surface_parameter_frame",
            exactness: Exactness::ByteExact,
        },
    )?;
    let nonvisible_surface_parameters = surface_parameter_records(
        ctx,
        scan,
        &scan.surfaces.nonvisible_rows,
        &scan.surfaces.nonvisible_parameters,
        "novisgeom",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::NonvisibleSurfaceParameters,
            records: &nonvisible_surface_parameters,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.body_offset),
            tag: "nonvisible_surface_parameter_frame",
            exactness: Exactness::ByteExact,
        },
    )?;
    let cross_section_surface_parameters = surface_parameter_records(
        ctx,
        scan,
        &scan.surfaces.cross_section_rows,
        &scan.surfaces.cross_section_parameters,
        "cross_section_geometry",
    )?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::CrossSectionSurfaceParameters,
            records: &cross_section_surface_parameters,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.body_offset),
            tag: "cross_section_surface_parameter_frame",
            exactness: Exactness::ByteExact,
        },
    )?;
    let plane_local_systems = plane_local_system_records(
        ctx,
        scan,
        &scan.planes.local_systems,
        "creo:surface:plane_local_system",
    )?;
    store_arena(ctx, ir, CreoArena::PlaneLocalSystems, &plane_local_systems)?;
    let cross_section_plane_local_systems = plane_local_system_records(
        ctx,
        scan,
        &scan.planes.cross_section_local_systems,
        "creo:cross_section_geometry:plane_local_system",
    )?;
    store_arena(
        ctx,
        ir,
        CreoArena::CrossSectionPlaneLocalSystems,
        &cross_section_plane_local_systems,
    )?;
    let plane_envelopes = plane_envelope_records(
        ctx,
        scan,
        &scan.planes.envelopes,
        "creo:surface:plane_envelope",
    )?;
    store_arena(ctx, ir, CreoArena::PlaneEnvelopes, &plane_envelopes)?;
    let cross_section_plane_envelopes = plane_envelope_records(
        ctx,
        scan,
        &scan.planes.cross_section_envelopes,
        "creo:cross_section_geometry:plane_envelope",
    )?;
    store_arena(
        ctx,
        ir,
        CreoArena::CrossSectionPlaneEnvelopes,
        &cross_section_plane_envelopes,
    )?;
    let outline_planes = outline_plane_records(
        ctx,
        scan,
        &scan.planes.outlines,
        "creo:surface:outline_plane",
    )?;
    store_arena(ctx, ir, CreoArena::OutlinePlanes, &outline_planes)?;
    let positional_frame_planes = outline_plane_records(
        ctx,
        scan,
        &scan.planes.positional_frames,
        "creo:surface:positional_frame_plane",
    )?;
    store_arena(
        ctx,
        ir,
        CreoArena::PositionalFramePlanes,
        &positional_frame_planes,
    )?;
    let cross_section_outline_planes = outline_plane_records(
        ctx,
        scan,
        &scan.planes.cross_section_outlines,
        "creo:cross_section_geometry:outline_plane",
    )?;
    store_arena(
        ctx,
        ir,
        CreoArena::CrossSectionOutlinePlanes,
        &cross_section_outline_planes,
    )?;
    let datum_planes = datum_plane_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::DatumPlanes, &datum_planes)?;
    let datum_cylinders = datum_cylinder_records(ctx, scan)?;
    store_arena(ctx, ir, CreoArena::DatumCylinders, &datum_cylinders)?;
    let feature_section_transforms = feature_section_transform_records(ctx, scan)?;
    store_arena(
        ctx,
        ir,
        CreoArena::FeatureSectionTransforms,
        &feature_section_transforms,
    )?;
    let feature_placement_instructions = feature_placement_instruction_records(ctx, scan)?;
    store_arena(
        ctx,
        ir,
        CreoArena::FeaturePlacementInstructions,
        &feature_placement_instructions,
    )?;
    // Bespoke annotation: the arena payload drops the per-record source offset the
    // annotation needs, so the offset travels alongside each record in a tuple.
    let pcurve_endpoints = pcurve_endpoint_records(ctx, scan)?;
    for (record, offset) in &pcurve_endpoints {
        annotate(
            ctx,
            annotations,
            &record.id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(*offset),
            "pcurve_endpoint_frames",
            Exactness::Derived,
        )?;
    }
    let mut pcurve_endpoint_payload = Vec::new();
    ctx.reserve_vec(
        &mut pcurve_endpoint_payload,
        pcurve_endpoints.len(),
        "creo native pcurve endpoint payload references",
    )?;
    pcurve_endpoint_payload.extend(pcurve_endpoints.iter().map(|(record, _)| record));
    store_arena(
        ctx,
        ir,
        CreoArena::PcurveEndpoints,
        &pcurve_endpoint_payload,
    )?;
    let feature_definitions = feature_definition_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureDefinitions,
            records: &feature_definitions,
            id: |definition| &definition.id,
            stream: |definition| definition.source_section,
            offset: |definition| cadmpeg_core::decode::u64_from_index(definition.offset),
            tag: "feature_definition_record",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_entities = feature_entity_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureEntities,
            records: &feature_entities,
            id: |entity| &entity.id,
            stream: |_| "AllFeatur",
            offset: |entity| cadmpeg_core::decode::u64_from_index(entity.offset),
            tag: "feature_entity",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_entity_references = feature_entity_reference_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureEntityReferences,
            records: &feature_entity_references,
            id: |reference| &reference.id,
            stream: |_| "AllFeatur",
            offset: |reference| cadmpeg_core::decode::u64_from_index(reference.offset),
            tag: "feature_entity_reference",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_entity_tables = feature_entity_table_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureEntityTables,
            records: &feature_entity_tables,
            id: |table| &table.id,
            stream: |_| "AllFeatur",
            offset: |table| cadmpeg_core::decode::u64_from_index(table.offset),
            tag: "feature_entity_table",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_surface_replays = feature_surface_replay_associations(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureSurfaceReplays,
            records: &feature_surface_replays,
            id: |association| &association.id,
            stream: |_| "AllFeatur",
            offset: |association| cadmpeg_core::decode::u64_from_index(association.table_offset),
            tag: "feature_surface_replay_association",
            exactness: Exactness::Derived,
        },
    )?;
    let feature_geometry_tables = feature_geometry_table_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureGeometryTables,
            records: &feature_geometry_tables,
            id: |table| &table.id,
            stream: |table| table.source_section,
            offset: |table| cadmpeg_core::decode::u64_from_index(table.offset),
            tag: "feature_geometry_table",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_loop_history_entries = feature_loop_history_entry_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureLoopHistoryEntries,
            records: &feature_loop_history_entries,
            id: |entry| &entry.id,
            stream: |entry| entry.source_section,
            offset: |entry| cadmpeg_core::decode::u64_from_index(entry.offset),
            tag: "feature_loop_history_entry",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_affected_ids = feature_affected_id_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureAffectedIds,
            records: &feature_affected_ids,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_affected_ids",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_replay_affected_ids = feature_replay_affected_id_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureReplayAffectedIds,
            records: &feature_replay_affected_ids,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_replay_affected_ids",
            exactness: Exactness::ByteExact,
        },
    )?;
    let surface_merge_replay_affected_ids = surface_merge_replay_affected_id_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::SurfaceMergeReplayAffectedIds,
            records: &surface_merge_replay_affected_ids,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "surface_merge_replay_affected_ids",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_loop_restore_directions = feature_loop_restore_direction_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureLoopRestoreDirections,
            records: &feature_loop_restore_directions,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_loop_restore_direction",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_revolution_extents = feature_revolution_extent_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureRevolutionExtents,
            records: &feature_revolution_extents,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_revolution_extent",
            exactness: Exactness::Derived,
        },
    )?;
    let feature_rows = feature_row_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureRows,
            records: &feature_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let depdb_recipe_rows = depdb_recipe_row_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::DepdbRecipeRows,
            records: &depdb_recipe_rows,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "depdb_recipe_row",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_choices = feature_choice_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureChoices,
            records: &feature_choices,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_choice",
            exactness: Exactness::ByteExact,
        },
    )?;
    let feature_choice_fields = feature_choice_field_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureChoiceFields,
            records: &feature_choice_fields,
            id: |record| &record.id,
            stream: |record| record.source_section,
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_choice_field",
            exactness: Exactness::ByteExact,
        },
    )?;
    let sketches = sketch_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::Sketches,
            records: &sketches,
            id: |sketch| &sketch.id,
            stream: |sketch| &sketch.source_section,
            offset: |sketch| cadmpeg_core::decode::u64_from_index(sketch.offset),
            tag: "feature_sketch",
            exactness: Exactness::Derived,
        },
    )?;
    // Bespoke annotation: the source offset comes from the parallel scan rows, not
    // the record, so annotation zips the two before the arena is stored.
    let curve_expressions = curve_expression_records(ctx, scan)?;
    for (expression, source) in curve_expressions.iter().zip(&scan.curves.expressions) {
        let source_section = source_section_ref(scan, source.expression_offset);
        annotate(
            ctx,
            annotations,
            &expression.id,
            source_section,
            cadmpeg_core::decode::u64_from_index(source.expression_offset),
            "curve_expression_program",
            Exactness::ByteExact,
        )?;
    }
    store_arena(ctx, ir, CreoArena::CurveExpressions, &curve_expressions)?;
    let feature_operation_states = feature_operation_state_records(ctx, scan)?;
    emit_arena(
        ctx,
        ir,
        annotations,
        CreoArena::FeatureOperationStates,
        &feature_operation_states,
        |annotations, state| {
            let section = scan
                .framing
                .sections
                .iter()
                .find(|section| section.contains(state.state_offset))
                .map_or("MdlStatus", |section| section.name());
            annotate(
                ctx,
                annotations,
                &state.id,
                section,
                cadmpeg_core::decode::u64_from_index(state.state_offset),
                "feature_operation_state",
                Exactness::ByteExact,
            )?;
            Ok(())
        },
    )?;
    let feature_reference_names = feature_reference_name_records(ctx, scan)?;
    emit_uniform(
        ctx,
        ir,
        annotations,
        &UniformArena {
            key: CreoArena::FeatureReferenceNames,
            records: &feature_reference_names,
            id: |record| &record.id,
            stream: |_| "MdlRefInfo",
            offset: |record| cadmpeg_core::decode::u64_from_index(record.offset),
            tag: "feature_reference_name",
            exactness: Exactness::ByteExact,
        },
    )?;
    if let Some(family_table) = family_table_record(scan) {
        annotate(
            ctx,
            annotations,
            super::super::records::CreoFamilyTableRecord::ID,
            "FamilyInf",
            cadmpeg_core::decode::u64_from_index(family_table.offset),
            "configuration_driver_table_pointer",
            Exactness::ByteExact,
        )?;
        store_arena(ctx, ir, CreoArena::Configuration, &[family_table])?;
    }
    Ok(())
}
