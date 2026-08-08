// SPDX-License-Identifier: Apache-2.0
//! Integration contracts over synthesized STEP Part 21 exchanges.

use super::*;

use cadmpeg_ir::codec::{EncodeInput, Encoder};
use std::fmt::Debug;

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir.native.namespace("step").is_some());
}

fn sorted_debug(values: impl IntoIterator<Item = impl Debug>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>();
    values.sort();
    values
}

/// Fingerprint the semantic geometry and topology without source or arena IDs.
/// The writer may assign different STEP instance numbers on re-decode, but it
/// must preserve these canonical carrier values, tolerances, and topology
/// cardinalities.
fn semantic_fingerprint(ir: &cadmpeg_ir::CadIr) -> String {
    let arena_counts = [
        ("bodies", ir.model.bodies.len()),
        ("regions", ir.model.regions.len()),
        ("shells", ir.model.shells.len()),
        ("faces", ir.model.faces.len()),
        ("loops", ir.model.loops.len()),
        ("coedges", ir.model.coedges.len()),
        ("edges", ir.model.edges.len()),
        ("vertices", ir.model.vertices.len()),
        ("points", ir.model.points.len()),
        ("surfaces", ir.model.surfaces.len()),
        ("curves", ir.model.curves.len()),
        ("pcurves", ir.model.pcurves.len()),
        ("procedural_surfaces", ir.model.procedural_surfaces.len()),
        ("procedural_curves", ir.model.procedural_curves.len()),
        ("tessellations", ir.model.tessellations.len()),
        ("appearances", ir.model.appearances.len()),
        ("appearance_bindings", ir.model.appearance_bindings.len()),
        ("pmi", ir.model.pmi.len()),
    ];
    let points = sorted_debug(ir.model.points.iter().map(|point| point.position));
    let curves = sorted_debug(ir.model.curves.iter().map(|curve| &curve.geometry));
    let surfaces = sorted_debug(ir.model.surfaces.iter().map(|surface| &surface.geometry));
    let pcurves = sorted_debug(ir.model.pcurves.iter().map(|pcurve| &pcurve.geometry));
    let bodies = sorted_debug(
        ir.model
            .bodies
            .iter()
            .map(|body| (&body.kind, body.regions.len())),
    );
    let regions = sorted_debug(ir.model.regions.iter().map(|region| region.shells.len()));
    let shells = sorted_debug(ir.model.shells.iter().map(|shell| {
        (
            shell.faces.len(),
            shell.wire_edges.len(),
            shell.free_vertices.len(),
        )
    }));
    let faces = sorted_debug(
        ir.model
            .faces
            .iter()
            .map(|face| (face.sense, face.loops.len(), face.tolerance)),
    );
    let loops = sorted_debug(ir.model.loops.iter().map(|loop_| {
        (
            loop_.boundary_role,
            loop_.coedges.len(),
            loop_.vertex_uses.len(),
        )
    }));
    let coedges = sorted_debug(ir.model.coedges.iter().map(|coedge| {
        (
            coedge.sense,
            coedge.pcurves.len(),
            coedge.use_curve.is_some(),
            coedge.use_curve_parameter_range,
        )
    }));
    let edges = sorted_debug(
        ir.model
            .edges
            .iter()
            .map(|edge| (edge.curve.is_some(), edge.tolerance)),
    );
    let vertices = sorted_debug(ir.model.vertices.iter().map(|vertex| vertex.tolerance));
    let semantic_entity_count = ir
        .model
        .entity_count()
        .saturating_sub(ir.model.product_definitions.len())
        .saturating_sub(ir.model.occurrences.len());
    format!(
        "ir_entities={};units={:?};tolerances={:?};arena_counts={arena_counts:?};points={points:?};curves={curves:?};surfaces={surfaces:?};pcurves={pcurves:?};bodies={bodies:?};regions={regions:?};shells={shells:?};faces={faces:?};loops={loops:?};coedges={coedges:?};edges={edges:?};vertices={vertices:?}",
        semantic_entity_count,
        ir.units,
        ir.tolerances,
    )
}

#[test]
fn part21_pipeline_composes_envelope_editions_strings_anchors_opaque_records_and_dependencies() {
    codec_detects_and_inspects_ap242_exchange_structure();
    codec_inspects_edition3_sections_and_external_references();
    string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode();
    decode_reports_data_section_external_dependencies();
    decode_preserves_named_opaque_records_with_exact_byte_spans();
    decode_accounts_for_every_part21_byte();
}

#[test]
fn geometry_pipeline_composes_analytic_conic_nurbs_procedural_and_unit_conversion_paths() {
    decode_transfers_placed_analytic_geometry_in_millimetres();
    decode_conical_apex_and_context_plane_angle_units();
    decode_resolves_conversion_units_and_linear_uncertainty();
    procedural_step_geometry_round_trips_as_native_entities();
    writer_round_trips_rational_nurbs_pcurves();
    analytic_conics_round_trip_through_step();
    nurbs_surface_grid_orientation_is_u_major();
}

#[test]
fn topology_pipeline_composes_sheets_solids_wires_singular_loops_bounds_and_placements() {
    decode_and_write_singular_vertex_loops();
    decode_builds_a_valid_connected_sheet_brep();
    decode_builds_a_valid_ap203_sheet_brep();
    reader_recovers_a_valid_solid_from_writer_output();
    writer_round_trips_rigid_body_placements();
    writer_round_trips_edge_based_wire_bodies();
    face_outer_bound_is_canonicalized_ahead_of_inner_bounds();
    every_region_of_a_body_is_retained_as_a_shape_item();
}

#[test]
fn product_pipeline_composes_body_ownership_occurrences_mappings_and_recursive_assemblies() {
    writer_round_trips_product_body_ownership();
    decode_builds_product_occurrences_with_relative_placement();
    decode_builds_occurrence_placement_from_mapped_item();
    repeated_subassembly_instances_each_receive_the_subtree();
    ap203_specified_source_formations_build_occurrence_tree();
    standalone_geometry_uses_general_shape_representation();
}

#[test]
fn presentation_pipeline_composes_tessellation_colors_visibility_layers_and_style_overrides() {
    ap242_writer_round_trips_indexed_tessellation_and_exact_body_link();
    step_color_assets_round_trip_names_and_tessellation_targets_strictly();
    decode_transfers_ap242_one_based_tessellation_indices();
    hidden_body_geometry_and_visibility_round_trip();
    body_color_becomes_per_face_styled_item_presentation();
    face_appearance_binding_styles_the_advanced_face();
    face_override_wins_over_body_color_and_body_fills_the_rest();
    presentation_reader_normalizes_invalid_layer_and_common_datum_inputs();
}

#[test]
fn pmi_pipeline_composes_semantic_presentation_dimension_datum_and_tolerance_entities() {
    decode_transfers_ap242_semantic_pmi();
    decode_transfers_ap242_presentation_pmi();
    ap242_dimension_kinds_emit_concrete_schema_entities();
    common_datum_compartment_round_trips_as_one_precedence();
    typed_pmi_measure_uses_its_explicit_conversion_unit();
    unresolved_lower_tolerance_does_not_shift_upper_deviation();
}

#[test]
fn writer_pipeline_round_trips_the_full_cube_across_schemas_and_refuses_lossy_strict_output() {
    let ir = unit_cube();
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        let options = StepWriteOptions {
            schema,
            ..StepWriteOptions::default()
        };
        let mut bytes = Vec::new();
        write_step(&ir, &mut bytes, &options).expect("STEP cube write");
        let mut repeated = Vec::new();
        write_step(&ir, &mut repeated, &options).expect("repeat STEP cube write");
        assert_eq!(
            bytes, repeated,
            "STEP output must be deterministic for {schema:?}"
        );
        assert_eq!(StepCodec::default().detect(&bytes), Confidence::High);
        let result = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("STEP cube decode");
        assert_eq!(result.ir.model.bodies.len(), 1);
        assert_eq!(result.ir.model.faces.len(), 6);
        assert_valid(&result);
        assert_eq!(
            semantic_fingerprint(&result.ir),
            semantic_fingerprint(&ir),
            "semantic fingerprint changed after {schema:?} re-decode"
        );

        let mut edited = result.ir.clone();
        edited
            .model
            .points
            .first_mut()
            .expect("unit cube has a point")
            .position
            .x += 1.0;
        let edited_fingerprint = semantic_fingerprint(&edited);
        let expected_model = edited.model.clone();
        let codec = StepCodec {
            options: options.clone(),
        };
        let plan = codec
            .plan(EncodeInput {
                ir: &edited,
                fidelity: Some(&result.source_fidelity),
            })
            .expect("edited STEP document plan");
        assert_eq!(plan.write_path(), cadmpeg_ir::WritePath::Synthesized);
        assert_eq!(
            plan.fidelity_resolution(),
            &cadmpeg_ir::FidelityResolution::NotConsumed
        );
        let mut edited_bytes = Vec::new();
        let export = plan
            .write_to(&mut edited_bytes)
            .expect("edited STEP document write");
        assert_eq!(export.write_path, cadmpeg_ir::WritePath::Synthesized);
        let edited_result = codec
            .decode(&mut Cursor::new(edited_bytes), &DecodeOptions::default())
            .expect("edited STEP document decode");
        assert_valid(&edited_result);
        assert_eq!(
            semantic_fingerprint(&edited_result.ir),
            edited_fingerprint,
            "edited semantic fingerprint changed after {schema:?} re-decode"
        );
        assert_eq!(
            edited_result.ir.model.bodies.len(),
            expected_model.bodies.len()
        );
        assert_eq!(
            edited_result.ir.model.faces.len(),
            expected_model.faces.len()
        );
        let expected_points = expected_model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>();
        let actual_points = edited_result
            .ir
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>();
        assert_eq!(actual_points.len(), expected_points.len());
        let mut matched = vec![false; actual_points.len()];
        for expected in expected_points {
            let actual = actual_points
                .iter()
                .enumerate()
                .find(|(index, actual)| {
                    !matched[*index] && actual.distance_squared(expected) <= 1.0e-18
                })
                .map(|(index, actual)| {
                    matched[index] = true;
                    *actual
                });
            assert!(
                actual.is_some(),
                "edited point was not preserved: {expected:?}"
            );
        }
    }
    strict_writer_rejects_before_emitting_bytes();
    strict_writer_refuses_retained_opaque_step_records_atomically();
    rejected_step_write_detects_incomplete_datum_system();
}
