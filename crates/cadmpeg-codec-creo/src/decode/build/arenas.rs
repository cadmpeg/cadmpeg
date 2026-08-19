// SPDX-License-Identifier: Apache-2.0
//! Native reference-geometry and model-namespace arena emission.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;

use super::super::coverage::source_section;
use super::super::expanded::{
    fc05_circle_records, fc05_cylinder_cap_pair_records, feature_surface_replay_associations,
};
use super::super::native::emit_arena;
use super::super::native::{annotate, emit_uniform, store_arena};
use super::super::records::family_table_record;
use super::super::records::{
    cross_section_curve_row_records, curve_expression_records, curve_parameter_records,
    curve_prototype_records, curve_prototype_topology_records, curve_topology_row_records,
    datum_plane_records, depdb_recipe_row_records, face_component_records,
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

/// Emit the `MdlRefInfo` reference-geometry arenas.
///
/// Reference lines, circles, conics, and ellipse carriers, each annotated
/// against the `MdlRefInfo` stream at the record offset.
pub(in super::super) fn emit_reference_arenas(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    emit_uniform(
        ir,
        annotations,
        "reference_lines",
        &reference_line_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_line_record",
        Exactness::ByteExact,
    )?;
    emit_uniform(
        ir,
        annotations,
        "reference_circles",
        &reference_circle_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_circle_record",
        Exactness::Derived,
    )?;
    emit_uniform(
        ir,
        annotations,
        "reference_conics",
        &reference_conic_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_conic_record",
        Exactness::ByteExact,
    )?;
    emit_uniform(
        ir,
        annotations,
        "reference_ellipses",
        &reference_ellipse_records(scan),
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "reference_ellipse_carrier",
        Exactness::Derived,
    )?;
    Ok(())
}

/// Emit the surface, curve, topology, plane, and feature arenas.
///
/// Each arena is built from the scan and stored under its native key in the
/// order the source streams are read; that order fixes the annotation stream
/// numbering, so the emissions must not be reordered.
pub(in super::super) fn emit_geometry_arenas(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let surface_rows = surface_row_records(scan, &scan.surfaces.rows, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "surface_rows",
        &surface_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_namespace_row",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_rows =
        surface_row_records(scan, &scan.surfaces.nonvisible_rows, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_rows",
        &nonvisible_surface_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_surface_namespace_row",
        Exactness::ByteExact,
    )?;
    let cross_section_surface_rows = surface_row_records(
        scan,
        &scan.surfaces.cross_section_rows,
        "cross_section_geometry",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_surface_rows",
        &cross_section_surface_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_surface_namespace_row",
        Exactness::ByteExact,
    )?;
    let surface_contours = surface_contour_records(scan, &scan.surfaces.contours, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "surface_contours",
        &surface_contours,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_contour_chain_entry",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_contours =
        surface_contour_records(scan, &scan.surfaces.nonvisible_contours, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_contours",
        &nonvisible_surface_contours,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_surface_contour_chain_entry",
        Exactness::ByteExact,
    )?;
    let cross_section_surface_contours = surface_contour_records(
        scan,
        &scan.surfaces.cross_section_contours,
        "cross_section_geometry",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_surface_contours",
        &cross_section_surface_contours,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_surface_contour_chain_entry",
        Exactness::ByteExact,
    )?;
    let surface_prototypes =
        surface_prototype_records(scan, &scan.surfaces.prototype_records, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "surface_prototypes",
        &surface_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_prototype_record",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_prototypes = surface_prototype_records(
        scan,
        &scan.surfaces.nonvisible_prototype_records,
        "novisgeom",
    );
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_prototypes",
        &nonvisible_surface_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_surface_prototype_record",
        Exactness::ByteExact,
    )?;
    let tabulated_cylinder_curve_replays = tabulated_cylinder_curve_replay_records(scan);
    emit_uniform(
        ir,
        annotations,
        "tabulated_cylinder_curve_replays",
        &tabulated_cylinder_curve_replays,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "tabulated_cylinder_curve_replay",
        Exactness::ByteExact,
    )?;
    let curve_parameters = curve_parameter_records(scan, &scan.curves.parameters, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "curve_parameters",
        &curve_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "curve_parameter_record",
        Exactness::ByteExact,
    )?;
    let nonvisible_curve_parameters =
        curve_parameter_records(scan, &scan.curves.nonvisible_parameters, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_curve_parameters",
        &nonvisible_curve_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_curve_parameter_record",
        Exactness::ByteExact,
    )?;
    let fc_curve_coordinates = fc_curve_coordinate_records(scan);
    emit_uniform(
        ir,
        annotations,
        "fc_curve_coordinates",
        &fc_curve_coordinates,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "fc_curve_coordinates",
        Exactness::ByteExact,
    )?;
    let fc05_circles = fc05_circle_records(scan);
    store_arena(ir, "fc05_circles", &fc05_circles)?;
    let fc05_cylinder_cap_pairs = fc05_cylinder_cap_pair_records(scan);
    store_arena(ir, "fc05_cylinder_cap_pairs", &fc05_cylinder_cap_pairs)?;
    let prototype_pcurves = prototype_pcurve_records(scan);
    store_arena(ir, "prototype_pcurves", &prototype_pcurves)?;
    let curve_prototype_topology = curve_prototype_topology_records(scan);
    store_arena(ir, "curve_prototype_topology", &curve_prototype_topology)?;
    let curve_prototypes =
        curve_prototype_records(scan, &scan.curves.prototypes, "creo:curve:prototype");
    emit_uniform(
        ir,
        annotations,
        "curve_prototypes",
        &curve_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "curve_prototype",
        Exactness::ByteExact,
    )?;
    let nonvisible_curve_prototypes = curve_prototype_records(
        scan,
        &scan.curves.nonvisible_prototypes,
        "creo:novisgeom:curve_prototype",
    );
    emit_uniform(
        ir,
        annotations,
        "nonvisible_curve_prototypes",
        &nonvisible_curve_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_curve_prototype",
        Exactness::ByteExact,
    )?;
    let cross_section_curve_prototypes = curve_prototype_records(
        scan,
        &scan.curves.cross_section_prototypes,
        "creo:cross_section_geometry:curve_prototype",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_curve_prototypes",
        &cross_section_curve_prototypes,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_curve_prototype",
        Exactness::ByteExact,
    )?;
    let curve_topology_rows =
        curve_topology_row_records(scan, &scan.curves.topology_rows, "visibgeom");
    emit_uniform(
        ir,
        annotations,
        "curve_topology_rows",
        &curve_topology_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "curve_topology_row",
        Exactness::ByteExact,
    )?;
    let nonvisible_curve_topology_rows =
        curve_topology_row_records(scan, &scan.curves.nonvisible_topology_rows, "novisgeom");
    emit_uniform(
        ir,
        annotations,
        "nonvisible_curve_topology_rows",
        &nonvisible_curve_topology_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "nonvisible_curve_topology_row",
        Exactness::ByteExact,
    )?;
    let cross_section_curve_rows = cross_section_curve_row_records(scan);
    emit_uniform(
        ir,
        annotations,
        "cross_section_curve_rows",
        &cross_section_curve_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "cross_section_curve_row",
        Exactness::ByteExact,
    )?;
    let loop_array_frames = loop_array_frame_records(scan);
    store_arena(ir, "loop_array_frames", &loop_array_frames)?;
    let loop_array_records = loop_array_record_records(scan);
    emit_uniform(
        ir,
        annotations,
        "loop_array_records",
        &loop_array_records,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "loop_array_record",
        Exactness::ByteExact,
    )?;
    let half_edges = half_edge_records(scan);
    emit_uniform(
        ir,
        annotations,
        "half_edges",
        &half_edges,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "native_half_edge",
        Exactness::Derived,
    )?;
    let native_loops = loop_records(scan);
    store_arena(ir, "loops", &native_loops)?;
    let topological_vertices = topological_vertex_records(scan);
    store_arena(ir, "topological_vertices", &topological_vertices)?;
    let half_edge_vertex_incidence = half_edge_vertex_incidence_records(scan);
    store_arena(
        ir,
        "half_edge_vertex_incidence",
        &half_edge_vertex_incidence,
    )?;
    let face_components = face_component_records(scan);
    store_arena(ir, "face_components", &face_components)?;
    let surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    );
    emit_uniform(
        ir,
        annotations,
        "surface_parameters",
        &surface_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.body_offset as u64,
        "surface_parameter_frame",
        Exactness::ByteExact,
    )?;
    let nonvisible_surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.nonvisible_rows,
        &scan.surfaces.nonvisible_parameters,
        "novisgeom",
    );
    emit_uniform(
        ir,
        annotations,
        "nonvisible_surface_parameters",
        &nonvisible_surface_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.body_offset as u64,
        "nonvisible_surface_parameter_frame",
        Exactness::ByteExact,
    )?;
    let cross_section_surface_parameters = surface_parameter_records(
        scan,
        &scan.surfaces.cross_section_rows,
        &scan.surfaces.cross_section_parameters,
        "cross_section_geometry",
    );
    emit_uniform(
        ir,
        annotations,
        "cross_section_surface_parameters",
        &cross_section_surface_parameters,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.body_offset as u64,
        "cross_section_surface_parameter_frame",
        Exactness::ByteExact,
    )?;
    let plane_local_systems = plane_local_system_records(
        scan,
        &scan.planes.local_systems,
        "creo:surface:plane_local_system",
    );
    store_arena(ir, "plane_local_systems", &plane_local_systems)?;
    let cross_section_plane_local_systems = plane_local_system_records(
        scan,
        &scan.planes.cross_section_local_systems,
        "creo:cross_section_geometry:plane_local_system",
    );
    store_arena(
        ir,
        "cross_section_plane_local_systems",
        &cross_section_plane_local_systems,
    )?;
    let plane_envelopes =
        plane_envelope_records(scan, &scan.planes.envelopes, "creo:surface:plane_envelope");
    store_arena(ir, "plane_envelopes", &plane_envelopes)?;
    let cross_section_plane_envelopes = plane_envelope_records(
        scan,
        &scan.planes.cross_section_envelopes,
        "creo:cross_section_geometry:plane_envelope",
    );
    store_arena(
        ir,
        "cross_section_plane_envelopes",
        &cross_section_plane_envelopes,
    )?;
    let outline_planes =
        outline_plane_records(scan, &scan.planes.outlines, "creo:surface:outline_plane");
    store_arena(ir, "outline_planes", &outline_planes)?;
    let positional_frame_planes = outline_plane_records(
        scan,
        &scan.planes.positional_frames,
        "creo:surface:positional_frame_plane",
    );
    store_arena(ir, "positional_frame_planes", &positional_frame_planes)?;
    let cross_section_outline_planes = outline_plane_records(
        scan,
        &scan.planes.cross_section_outlines,
        "creo:cross_section_geometry:outline_plane",
    );
    store_arena(
        ir,
        "cross_section_outline_planes",
        &cross_section_outline_planes,
    )?;
    let datum_planes = datum_plane_records(scan);
    store_arena(ir, "datum_planes", &datum_planes)?;
    let feature_section_transforms = feature_section_transform_records(scan);
    store_arena(
        ir,
        "feature_section_transforms",
        &feature_section_transforms,
    )?;
    let feature_placement_instructions = feature_placement_instruction_records(scan);
    store_arena(
        ir,
        "feature_placement_instructions",
        &feature_placement_instructions,
    )?;
    // Bespoke annotation: the arena payload drops the per-record source offset the
    // annotation needs, so the offset travels alongside each record in a tuple.
    let pcurve_endpoints = pcurve_endpoint_records(scan);
    for (record, offset) in &pcurve_endpoints {
        annotate(
            annotations,
            &record.id,
            "VisibGeom",
            *offset as u64,
            "pcurve_endpoint_frames",
            Exactness::Derived,
        );
    }
    let pcurve_endpoint_payload = pcurve_endpoints
        .iter()
        .map(|(record, _)| record)
        .collect::<Vec<_>>();
    store_arena(ir, "pcurve_endpoints", &pcurve_endpoint_payload)?;
    let feature_definitions = feature_definition_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_definitions",
        &feature_definitions,
        |definition| &definition.id,
        |definition| &definition.source_section,
        |definition| definition.offset as u64,
        "feature_definition_record",
        Exactness::ByteExact,
    )?;
    let feature_entities = feature_entity_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_entities",
        &feature_entities,
        |entity| &entity.id,
        |_| "AllFeatur",
        |entity| entity.offset as u64,
        "feature_entity",
        Exactness::ByteExact,
    )?;
    let feature_entity_references = feature_entity_reference_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_entity_references",
        &feature_entity_references,
        |reference| &reference.id,
        |_| "AllFeatur",
        |reference| reference.offset as u64,
        "feature_entity_reference",
        Exactness::ByteExact,
    )?;
    let feature_entity_tables = feature_entity_table_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_entity_tables",
        &feature_entity_tables,
        |table| &table.id,
        |_| "AllFeatur",
        |table| table.offset as u64,
        "feature_entity_table",
        Exactness::ByteExact,
    )?;
    let feature_surface_replays = feature_surface_replay_associations(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_surface_replays",
        &feature_surface_replays,
        |association| &association.id,
        |_| "AllFeatur",
        |association| association.table_offset as u64,
        "feature_surface_replay_association",
        Exactness::Derived,
    )?;
    let feature_geometry_tables = feature_geometry_table_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_geometry_tables",
        &feature_geometry_tables,
        |table| &table.id,
        |table| &table.source_section,
        |table| table.offset as u64,
        "feature_geometry_table",
        Exactness::ByteExact,
    )?;
    let feature_loop_history_entries = feature_loop_history_entry_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_loop_history_entries",
        &feature_loop_history_entries,
        |entry| &entry.id,
        |entry| &entry.source_section,
        |entry| entry.offset as u64,
        "feature_loop_history_entry",
        Exactness::ByteExact,
    )?;
    let feature_affected_ids = feature_affected_id_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_affected_ids",
        &feature_affected_ids,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_affected_ids",
        Exactness::ByteExact,
    )?;
    let feature_replay_affected_ids = feature_replay_affected_id_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_replay_affected_ids",
        &feature_replay_affected_ids,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_replay_affected_ids",
        Exactness::ByteExact,
    )?;
    let surface_merge_replay_affected_ids = surface_merge_replay_affected_id_records(scan);
    emit_uniform(
        ir,
        annotations,
        "surface_merge_replay_affected_ids",
        &surface_merge_replay_affected_ids,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "surface_merge_replay_affected_ids",
        Exactness::ByteExact,
    )?;
    let feature_loop_restore_directions = feature_loop_restore_direction_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_loop_restore_directions",
        &feature_loop_restore_directions,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_loop_restore_direction",
        Exactness::ByteExact,
    )?;
    let feature_revolution_extents = feature_revolution_extent_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_revolution_extents",
        &feature_revolution_extents,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_revolution_extent",
        Exactness::Derived,
    )?;
    let feature_rows = feature_row_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_rows",
        &feature_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_row",
        Exactness::ByteExact,
    )?;
    let depdb_recipe_rows = depdb_recipe_row_records(scan);
    emit_uniform(
        ir,
        annotations,
        "depdb_recipe_rows",
        &depdb_recipe_rows,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "depdb_recipe_row",
        Exactness::ByteExact,
    )?;
    let feature_choices = feature_choice_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_choices",
        &feature_choices,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_choice",
        Exactness::ByteExact,
    )?;
    let feature_choice_fields = feature_choice_field_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_choice_fields",
        &feature_choice_fields,
        |record| &record.id,
        |record| &record.source_section,
        |record| record.offset as u64,
        "feature_choice_field",
        Exactness::ByteExact,
    )?;
    let sketches = sketch_records(scan);
    emit_uniform(
        ir,
        annotations,
        "sketches",
        &sketches,
        |sketch| &sketch.id,
        |sketch| &sketch.source_section,
        |sketch| sketch.offset as u64,
        "feature_sketch",
        Exactness::Derived,
    )?;
    // Bespoke annotation: the source offset comes from the parallel scan rows, not
    // the record, so annotation zips the two before the arena is stored.
    let curve_expressions = curve_expression_records(scan);
    for (expression, source) in curve_expressions.iter().zip(&scan.curves.expressions) {
        let source_section = source_section(scan, source.expression_offset);
        annotate(
            annotations,
            &expression.id,
            &source_section,
            source.expression_offset as u64,
            "curve_expression_program",
            Exactness::ByteExact,
        );
    }
    store_arena(ir, "curve_expressions", &curve_expressions)?;
    let feature_operation_states = feature_operation_state_records(scan);
    emit_arena(
        ir,
        annotations,
        "feature_operation_states",
        &feature_operation_states,
        |annotations, state| {
            let section = scan
                .framing
                .sections
                .iter()
                .find(|section| {
                    state.state_offset >= section.offset
                        && state.state_offset < section.offset.saturating_add(section.length)
                })
                .map_or("MdlStatus", |section| section.name.as_str());
            annotate(
                annotations,
                &state.id,
                section,
                state.state_offset as u64,
                "feature_operation_state",
                Exactness::ByteExact,
            );
        },
    )?;
    let feature_reference_names = feature_reference_name_records(scan);
    emit_uniform(
        ir,
        annotations,
        "feature_reference_names",
        &feature_reference_names,
        |record| &record.id,
        |_| "MdlRefInfo",
        |record| record.offset as u64,
        "feature_reference_name",
        Exactness::ByteExact,
    )?;
    if let Some(family_table) = family_table_record(scan) {
        annotate(
            annotations,
            family_table.id,
            "FamilyInf",
            family_table.offset as u64,
            "configuration_driver_table_pointer",
            Exactness::ByteExact,
        );
        store_arena(ir, "configuration", &[family_table])?;
    }
    Ok(())
}
