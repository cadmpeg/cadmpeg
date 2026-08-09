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

#[derive(Clone, Copy, Debug)]
enum ExpectedArena {
    ModelBodies,
    ModelCurves,
    ModelPoints,
    ModelPcurves,
    ModelProceduralCurves,
    ModelSurfaces,
    Native(&'static str),
}

fn arena_count(result: &cadmpeg_ir::codec::DecodeResult, arena: ExpectedArena) -> usize {
    match arena {
        ExpectedArena::ModelBodies => result.ir.model.bodies.len(),
        ExpectedArena::ModelCurves => result.ir.model.curves.len(),
        ExpectedArena::ModelPoints => result.ir.model.points.len(),
        ExpectedArena::ModelPcurves => result.ir.model.pcurves.len(),
        ExpectedArena::ModelProceduralCurves => result.ir.model.procedural_curves.len(),
        ExpectedArena::ModelSurfaces => result.ir.model.surfaces.len(),
        ExpectedArena::Native(name) => result
            .ir
            .native
            .namespace("iges")
            .and_then(|namespace| namespace.arenas.get(name))
            .map_or(0, Vec::len),
    }
}

fn decode_matrix(
    fixtures: Vec<(&'static str, Vec<u8>, ExpectedArena)>,
) -> Vec<cadmpeg_ir::codec::DecodeResult> {
    fixtures
        .into_iter()
        .map(|(name, bytes, expected_arena)| {
            let result = decode(bytes);
            assert_valid(&result);
            assert!(
                arena_count(&result, expected_arena) > 0,
                "fixture {name} did not populate expected arena {expected_arena:?}"
            );
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
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("IGES inspection");
    assert_eq!(summary.format, "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert!(summary.notes.iter().any(|note| note.contains("5.3")));
    decode_matrix(vec![
        (
            "nested_transformed_point",
            bytes,
            ExpectedArena::ModelPoints,
        ),
        ("point", point_file(), ExpectedArena::ModelPoints),
        (
            "direction",
            direction_file(),
            ExpectedArena::Native("directions"),
        ),
        (
            "units_data",
            units_data_file(),
            ExpectedArena::Native("units_data"),
        ),
    ]);
}

#[test]
fn curve_pipeline_composes_analytic_conic_spline_composite_copious_and_offset_entities() {
    decode_matrix(vec![
        ("line", line_file(0), ExpectedArena::ModelCurves),
        (
            "circular_arc",
            circular_arc_file(),
            ExpectedArena::ModelCurves,
        ),
        (
            "nurbs_curve",
            nurbs_curve_file(),
            ExpectedArena::ModelCurves,
        ),
        (
            "rational_nurbs_curve",
            rational_nurbs_curve_file(),
            ExpectedArena::ModelCurves,
        ),
        (
            "parametric_spline_curve",
            parametric_spline_curve_file(),
            ExpectedArena::ModelCurves,
        ),
        (
            "mixed_analytic_composite_curve",
            mixed_analytic_composite_curve_file(),
            ExpectedArena::ModelCurves,
        ),
        (
            "uniform_offset_circle",
            uniform_offset_circle_file(),
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "linear_offset_line",
            linear_offset_line_file(1),
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "function_offset_line",
            function_offset_line_file(),
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "copious_polyline",
            copious_data_file(11, b"106,1,2,0,0,0,1,0;", "00000000"),
            ExpectedArena::ModelCurves,
        ),
    ]);
}

#[test]
fn surface_pipeline_composes_nurbs_power_patches_sweeps_revolution_offsets_and_trims() {
    decode_matrix(vec![
        (
            "nurbs_surface",
            nurbs_surface_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "parametric_spline_surface",
            parametric_spline_surface_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "ruled_surface",
            ruled_surface_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "tabulated_cylinder",
            tabulated_cylinder_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "surface_of_revolution",
            surface_of_revolution_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "placed_surface_of_revolution",
            placed_surface_of_revolution_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "offset_plane",
            offset_plane_file(1.0, 2.0),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "parametrically_bounded_plane",
            parametrically_bounded_plane_file(),
            ExpectedArena::ModelSurfaces,
        ),
        (
            "trimmed_plane_with_inner_loop",
            trimmed_plane_with_inner_loop_file(),
            ExpectedArena::ModelSurfaces,
        ),
    ]);
}

#[test]
fn topology_pipeline_composes_manifold_nonmanifold_void_seam_and_boundary_graphs() {
    let (void_solid, _, _, _) = explicit_void_solid_file();
    decode_matrix(vec![
        (
            "explicit_tetrahedron_solid",
            explicit_tetrahedron_solid_file(),
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_open_shell",
            explicit_open_shell_file(),
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_non_manifold_open_shell",
            explicit_non_manifold_open_shell_file(),
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_vertex_loop",
            explicit_vertex_loop_file(),
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_void_solid",
            void_solid,
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_multi_pcurve_loop",
            explicit_multi_pcurve_loop_file(),
            ExpectedArena::ModelPcurves,
        ),
        (
            "explicit_cylinder_seam",
            explicit_cylinder_seam_file(),
            ExpectedArena::ModelBodies,
        ),
        (
            "multi_pcurve_boundary",
            multi_pcurve_boundary_file(),
            ExpectedArena::ModelPcurves,
        ),
    ]);
}

#[test]
fn structure_pipeline_composes_csg_products_instances_patterns_groups_and_external_links() {
    decode_matrix(vec![
        (
            "primitive_solids",
            primitive_solids_file(),
            ExpectedArena::Native("primitive_solids"),
        ),
        (
            "procedural_and_boolean_solids",
            procedural_and_boolean_solids_file(),
            ExpectedArena::Native("procedural_solids"),
        ),
        (
            "solid_assembly",
            solid_assembly_file(),
            ExpectedArena::Native("solid_assemblies"),
        ),
        (
            "solid_instance",
            solid_instance_file(),
            ExpectedArena::Native("solid_instances"),
        ),
        (
            "patterned_instance",
            patterned_instance_file(),
            ExpectedArena::Native("rectangular_arrays"),
        ),
        (
            "external_reference_forms",
            external_reference_forms_file(),
            ExpectedArena::Native("external_references"),
        ),
        (
            "group_forms",
            group_forms_file(),
            ExpectedArena::Native("groups"),
        ),
        (
            "nested_subfigure",
            nested_subfigure_file(),
            ExpectedArena::Native("subfigure_definitions"),
        ),
        (
            "network_subfigure",
            network_subfigure_file(),
            ExpectedArena::Native("network_definitions"),
        ),
        (
            "connected_network_subfigure",
            connected_network_subfigure_file(),
            ExpectedArena::Native("network_instances"),
        ),
    ]);
}

#[test]
fn drawing_pipeline_composes_views_visibility_notes_leaders_dimensions_symbols_and_fonts() {
    decode_matrix(vec![
        (
            "view_forms",
            view_forms_file(),
            ExpectedArena::Native("views"),
        ),
        (
            "view_visibility_forms",
            view_visibility_forms_file(),
            ExpectedArena::Native("view_visibility"),
        ),
        (
            "segmented_view_visibility",
            segmented_view_visibility_file(),
            ExpectedArena::Native("segmented_visibility"),
        ),
        (
            "drawing_with_properties",
            drawing_with_properties_file(),
            ExpectedArena::Native("drawings"),
        ),
        (
            "text_annotation",
            text_annotation_file(),
            ExpectedArena::Native("annotations"),
        ),
        (
            "leader_forms",
            leader_forms_file(),
            ExpectedArena::Native("annotations"),
        ),
        (
            "dimension_forms",
            dimension_forms_file(),
            ExpectedArena::Native("annotations"),
        ),
        (
            "legacy_dimension_and_label_forms",
            legacy_dimension_and_label_forms_file(),
            ExpectedArena::Native("annotations"),
        ),
        (
            "symbol_and_sectioned_area",
            symbol_and_sectioned_area_file(),
            ExpectedArena::Native("annotations"),
        ),
        (
            "text_display_template_forms",
            text_display_template_forms_file(),
            ExpectedArena::Native("text_templates"),
        ),
        (
            "text_font_definition",
            text_font_definition_file(),
            ExpectedArena::Native("text_fonts"),
        ),
    ]);
}

#[test]
fn metadata_pipeline_composes_properties_attributes_associativity_and_native_ownership() {
    decode_matrix(vec![
        (
            "attribute_definition_forms",
            attribute_definition_forms_file(),
            ExpectedArena::Native("attribute_table_definitions"),
        ),
        (
            "attribute_instance_forms",
            attribute_instance_forms_file(),
            ExpectedArena::Native("attribute_table_instances"),
        ),
        (
            "product_property",
            product_property_file(),
            ExpectedArena::Native("product_properties"),
        ),
        (
            "scalar_property_forms",
            scalar_property_forms_file(),
            ExpectedArena::Native("properties"),
        ),
        (
            "grid_property",
            grid_property_file(),
            ExpectedArena::Native("properties"),
        ),
        (
            "dimension_property_forms",
            dimension_property_forms_file(),
            ExpectedArena::Native("properties"),
        ),
        (
            "drawing_metadata_property",
            drawing_metadata_property_forms_file(),
            ExpectedArena::Native("properties"),
        ),
        (
            "associativity_definition",
            associativity_definition_file(),
            ExpectedArena::Native("associativities"),
        ),
        (
            "bounded_associativity_forms",
            bounded_associativity_forms_file(),
            ExpectedArena::Native("associativities"),
        ),
        (
            "view_list_associativity",
            view_list_associativity_file(true),
            ExpectedArena::Native("associativities"),
        ),
        (
            "flow_associativity",
            flow_associativity_forms_file(),
            ExpectedArena::Native("associativities"),
        ),
        (
            "recalculable_dimension_associativity",
            recalculable_dimension_associativity_file(),
            ExpectedArena::Native("associativities"),
        ),
    ]);
}
