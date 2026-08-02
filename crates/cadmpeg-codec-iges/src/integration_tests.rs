// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized IGES 5.3 card streams.

use super::*;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    assert_eq!(IgesCodec.detect(&bytes), Confidence::High);
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized IGES stream should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir.native.namespace("iges").is_some());
}

fn decode_matrix(fixtures: Vec<Vec<u8>>) -> Vec<cadmpeg_ir::codec::DecodeResult> {
    fixtures
        .into_iter()
        .map(|bytes| {
            let result = decode(bytes);
            assert_valid(&result);
            result
        })
        .collect()
}

#[test]
fn envelope_pipeline_aligns_cards_global_units_directories_transforms_and_inspection() {
    let bytes = nested_transformed_point_file();
    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_codec_core::decode::InspectOptions::default(),
        )
        .expect("IGES inspection");
    assert_eq!(summary.format, "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert!(summary.notes.iter().any(|note| note.contains("5.3")));
    let results = decode_matrix(vec![
        bytes,
        point_file(),
        direction_file(),
        units_data_file(),
    ]);
    assert!(results
        .iter()
        .any(|result| !result.ir.model.points.is_empty()));
}

#[test]
fn curve_pipeline_composes_analytic_conic_spline_composite_copious_and_offset_entities() {
    let results = decode_matrix(vec![
        line_file(0),
        circular_arc_file(),
        nurbs_curve_file(),
        rational_nurbs_curve_file(),
        parametric_spline_curve_file(),
        mixed_analytic_composite_curve_file(),
        uniform_offset_circle_file(),
        linear_offset_line_file(1),
        function_offset_line_file(),
        copious_data_file(11, b"106,3,0.,0.,1.,0.;", "00000000"),
    ]);
    assert!(results
        .iter()
        .any(|result| !result.ir.model.curves.is_empty()));
    assert!(results
        .iter()
        .any(|result| !result.ir.model.procedural_curves.is_empty()));
}

#[test]
fn surface_pipeline_composes_nurbs_power_patches_sweeps_revolution_offsets_and_trims() {
    let results = decode_matrix(vec![
        nurbs_surface_file(),
        parametric_spline_surface_file(),
        ruled_surface_file(),
        tabulated_cylinder_file(),
        surface_of_revolution_file(),
        placed_surface_of_revolution_file(),
        offset_plane_file(1.0, 2.0),
        parametrically_bounded_plane_file(),
        trimmed_plane_with_inner_loop_file(),
    ]);
    assert!(results.iter().all(|result| {
        !result.ir.model.surfaces.is_empty() || !result.ir.model.procedural_surfaces.is_empty()
    }));
}

#[test]
fn topology_pipeline_composes_manifold_nonmanifold_void_seam_and_boundary_graphs() {
    let (void_solid, _, _, _) = explicit_void_solid_file();
    let results = decode_matrix(vec![
        explicit_tetrahedron_solid_file(),
        explicit_open_shell_file(),
        explicit_non_manifold_open_shell_file(),
        explicit_vertex_loop_file(),
        void_solid,
        explicit_multi_pcurve_loop_file(),
        explicit_cylinder_seam_file(),
        multi_pcurve_boundary_file(),
    ]);
    assert!(results
        .iter()
        .any(|result| !result.ir.model.bodies.is_empty()));
    assert!(results
        .iter()
        .any(|result| !result.ir.model.pcurves.is_empty()));
}

#[test]
fn structure_pipeline_composes_csg_products_instances_patterns_groups_and_external_links() {
    let results = decode_matrix(vec![
        primitive_solids_file(),
        procedural_and_boolean_solids_file(),
        solid_assembly_file(),
        solid_instance_file(),
        patterned_instance_file(),
        external_reference_forms_file(),
        group_forms_file(),
        nested_subfigure_file(),
        network_subfigure_file(),
        connected_network_subfigure_file(),
    ]);
    assert!(results.iter().any(|result| {
        let native = result.ir.native.namespace("iges").unwrap();
        !native.arenas["primitive_solids"].is_empty()
            || !native.arenas["procedural_solids"].is_empty()
            || !native.arenas["boolean_trees"].is_empty()
    }));
    assert!(results.iter().any(|result| {
        let native = result.ir.native.namespace("iges").unwrap();
        !native.arenas["solid_assemblies"].is_empty()
            || !native.arenas["product_occurrences"].is_empty()
            || !native.arenas["external_references"].is_empty()
    }));
}

#[test]
fn drawing_pipeline_composes_views_visibility_notes_leaders_dimensions_symbols_and_fonts() {
    let results = decode_matrix(vec![
        view_forms_file(),
        view_visibility_forms_file(),
        segmented_view_visibility_file(),
        drawing_with_properties_file(),
        text_annotation_file(),
        leader_forms_file(),
        dimension_forms_file(),
        legacy_dimension_and_label_forms_file(),
        symbol_and_sectioned_area_file(),
        text_display_template_forms_file(),
        text_font_definition_file(),
    ]);
    assert!(results.iter().any(|result| {
        !result.ir.native.namespace("iges").unwrap().arenas["drawings"].is_empty()
    }));
    assert!(results.iter().any(|result| {
        !result.ir.native.namespace("iges").unwrap().arenas["annotations"].is_empty()
    }));
}

#[test]
fn metadata_pipeline_composes_properties_attributes_associativity_and_native_ownership() {
    let results = decode_matrix(vec![
        attribute_definition_forms_file(),
        attribute_instance_forms_file(),
        product_property_file(),
        scalar_property_forms_file(),
        grid_property_file(),
        dimension_property_forms_file(),
        drawing_metadata_property_forms_file(),
        associativity_definition_file(),
        bounded_associativity_forms_file(),
        view_list_associativity_file(true),
        flow_associativity_forms_file(),
        recalculable_dimension_associativity_file(),
    ]);
    assert!(results.iter().all(|result| {
        result
            .ir
            .native
            .namespace("iges")
            .is_some_and(|namespace| !namespace.arenas["entities"].is_empty())
    }));
}
