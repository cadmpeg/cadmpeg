// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized IGES card streams.
#![allow(clippy::unwrap_used)]

use super::*;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::*;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    assert_eq!(IgesCodec.detect(&bytes), Confidence::High);
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized IGES stream should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir().native.namespace("iges").is_some());
}

#[derive(Clone, Copy, Debug)]
enum ExpectedArena {
    ModelBodies,
    ModelCoedges,
    ModelCurves,
    ModelLoops,
    ModelPoints,
    ModelPcurves,
    ModelProceduralCurves,
    ModelProceduralSurfaces,
    ModelRegions,
    ModelShells,
    ModelSurfaces,
    Native(&'static str),
}

fn arena_count(result: &cadmpeg_ir::codec::DecodeResult, arena: ExpectedArena) -> usize {
    match arena {
        ExpectedArena::ModelBodies => result.ir().model.bodies.len(),
        ExpectedArena::ModelCoedges => result.ir().model.coedges.len(),
        ExpectedArena::ModelCurves => result.ir().model.curves.len(),
        ExpectedArena::ModelLoops => result.ir().model.loops.len(),
        ExpectedArena::ModelPoints => result.ir().model.points.len(),
        ExpectedArena::ModelPcurves => result.ir().model.pcurves.len(),
        ExpectedArena::ModelProceduralCurves => result.ir().model.procedural_curves.len(),
        ExpectedArena::ModelProceduralSurfaces => result.ir().model.procedural_surfaces.len(),
        ExpectedArena::ModelRegions => result.ir().model.regions.len(),
        ExpectedArena::ModelShells => result.ir().model.shells.len(),
        ExpectedArena::ModelSurfaces => result.ir().model.surfaces.len(),
        ExpectedArena::Native(name) => result
            .ir()
            .native
            .namespace("iges")
            .and_then(|namespace| namespace.arenas.get(name))
            .map_or(0, Vec::len),
    }
}

fn arena_ids(result: &cadmpeg_ir::codec::DecodeResult, arena: ExpectedArena) -> Vec<&str> {
    match arena {
        ExpectedArena::ModelBodies => result
            .ir()
            .model
            .bodies
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelCoedges => result
            .ir()
            .model
            .coedges
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelCurves => result
            .ir()
            .model
            .curves
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelLoops => result
            .ir()
            .model
            .loops
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelPoints => result
            .ir()
            .model
            .points
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelPcurves => result
            .ir()
            .model
            .pcurves
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelProceduralCurves => result
            .ir()
            .model
            .procedural_curves
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelProceduralSurfaces => result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelRegions => result
            .ir()
            .model
            .regions
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelShells => result
            .ir()
            .model
            .shells
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::ModelSurfaces => result
            .ir()
            .model
            .surfaces
            .iter()
            .map(|item| item.id.0.as_str())
            .collect(),
        ExpectedArena::Native(name) => result
            .ir()
            .native
            .namespace("iges")
            .and_then(|namespace| namespace.arenas.get(name))
            .into_iter()
            .flatten()
            .map(cadmpeg_ir::NativeRecord::id)
            .collect(),
    }
}

fn identity_mentions_sequence(identity: &str, sequence: u32) -> bool {
    identity.match_indices('D').any(|(index, _)| {
        matches!(
            identity.as_bytes().get(index.wrapping_sub(1)),
            Some(b'#' | b':')
        ) && identity[index + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .ok()
            == Some(sequence)
    })
}

fn expected_counts(name: &str) -> (usize, usize, usize) {
    match name {
        "line"
        | "circular_arc"
        | "nurbs_curve"
        | "rational_nurbs_curve"
        | "parametric_spline_curve"
        | "mixed_analytic_composite_curve"
        | "uniform_offset_circle"
        | "linear_offset_line"
        | "function_offset_line"
        | "copious_polyline"
        | "segmented_view_visibility"
        | "drawing_with_properties"
        | "nested_transformed_point"
        | "point"
        | "direction"
        | "units_data"
        | "grid_property"
        | "associativity_definition"
        | "view_list_associativity"
        | "recalculable_dimension_associativity"
        | "solid_assembly"
        | "solid_instance"
        | "patterned_instance"
        | "network_subfigure"
        | "connected_network_subfigure"
        | "nurbs_surface"
        | "parametric_spline_surface"
        | "ruled_surface"
        | "tabulated_cylinder"
        | "surface_of_revolution"
        | "placed_surface_of_revolution"
        | "offset_plane"
        | "explicit_vertex_loop" => (1, 1, 1),
        "view_forms" | "attribute_definition_forms" => (3, 3, 3),
        "view_visibility_forms"
        | "attribute_instance_forms"
        | "product_property"
        | "drawing_metadata_property"
        | "group_forms"
        | "nested_subfigure"
        | "text_display_template_forms"
        | "text_font_definition" => (2, 2, 2),
        "text_annotation" => (1, 2, 1),
        "leader_forms" => (12, 12, 12),
        "dimension_forms" => (3, 11, 3),
        "legacy_dimension_and_label_forms" => (1, 8, 1),
        "symbol_and_sectioned_area" => (1, 4, 1),
        "scalar_property_forms" => (15, 15, 15),
        "dimension_property_forms" | "flow_associativity" => (4, 4, 4),
        "bounded_associativity_forms" => (6, 6, 6),
        "external_reference_forms" => (5, 5, 5),
        "primitive_solids" => (1, 8, 1),
        "procedural_and_boolean_solids" => (1, 3, 1),
        "parametrically_bounded_plane"
        | "trimmed_plane_with_inner_loop"
        | "explicit_tetrahedron_solid"
        | "explicit_open_shell"
        | "explicit_non_manifold_open_shell"
        | "explicit_void_solid"
        | "explicit_cylinder_seam" => (1, 2, 1),
        "explicit_multi_pcurve_loop" => (1, 2, 2),
        // Type 141 output identities are rooted at the owning Type 143.
        "multi_pcurve_boundary" => (1, 1, 0),
        _ => panic!("fixture {name} has no exact arena expectation"),
    }
}

fn decode_matrix(
    fixtures: Vec<(&'static str, Vec<u8>, i64, ExpectedArena)>,
) -> Vec<cadmpeg_ir::codec::DecodeResult> {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    fixtures
        .into_iter()
        .map(|(name, bytes, subject_type, expected_arena)| {
            assert_matrix_destination(&matrix, subject_type, expected_arena);
            let (expected_subjects, expected_total, expected_associated) = expected_counts(name);
            let scan = crate::card::scan(&bytes).expect("integration fixture cards");
            let (global, _global_losses) = crate::global::parse(&scan).expect("integration global");
            let (directory, _quarantined) = crate::directory::parse(&scan, global.global_table());
            let subject_count = directory
                .iter()
                .filter(|entry| entry.entity_type == subject_type)
                .count();
            let subject_sequences = directory
                .iter()
                .filter(|entry| entry.entity_type == subject_type)
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>();
            let result = decode(bytes);
            let subject_output_count = arena_ids(&result, expected_arena)
                .into_iter()
                .filter(|identity| {
                    subject_sequences
                        .iter()
                        .any(|sequence| identity_mentions_sequence(identity, *sequence))
                })
                .count();
            assert_eq!(
                subject_count, expected_subjects,
                "fixture {name} subject entity count"
            );
            assert_eq!(
                arena_count(&result, expected_arena),
                expected_total,
                "fixture {name} exact arena count for {expected_arena:?}"
            );
            assert_eq!(
                subject_output_count, expected_associated,
                "fixture {name} outputs associated with entity type {subject_type}"
            );
            assert_valid(&result);
            result
        })
        .collect()
}

fn matrix_destination(arena: ExpectedArena) -> &'static str {
    match arena {
        ExpectedArena::ModelBodies => "model.bodies",
        ExpectedArena::ModelCoedges => "model.coedges",
        ExpectedArena::ModelCurves => "model.curves",
        ExpectedArena::ModelLoops => "model.loops",
        ExpectedArena::ModelPoints => "model.points",
        ExpectedArena::ModelPcurves => "model.pcurves",
        ExpectedArena::ModelProceduralCurves => "model.procedural_curves",
        ExpectedArena::ModelProceduralSurfaces => "model.procedural_surfaces",
        ExpectedArena::ModelRegions => "model.regions",
        ExpectedArena::ModelShells => "model.shells",
        ExpectedArena::ModelSurfaces => "model.surfaces",
        ExpectedArena::Native("directions") => "native.iges.entities",
        ExpectedArena::Native("units_data" | "properties") => "native.iges.properties",
        ExpectedArena::Native("primitive_solids" | "procedural_solids") => "native.iges.solids",
        ExpectedArena::Native("solid_assemblies" | "subfigure_definitions") => {
            "native.iges.product"
        }
        ExpectedArena::Native("solid_instances" | "network_definitions" | "network_instances") => {
            "native.iges.product"
        }
        ExpectedArena::Native("rectangular_arrays" | "external_references") => {
            "native.iges.product"
        }
        ExpectedArena::Native("groups" | "associativities") => "native.iges.associativities",
        ExpectedArena::Native("views") => "native.iges.views",
        ExpectedArena::Native("view_visibility" | "segmented_visibility") => {
            "native.iges.associativities"
        }
        ExpectedArena::Native("drawings") => "native.iges.drawings",
        ExpectedArena::Native("annotations") => "native.iges.annotations",
        ExpectedArena::Native("text_templates" | "text_fonts") => "native.iges.presentation",
        ExpectedArena::Native("attribute_table_definitions" | "attribute_table_instances") => {
            "native.iges.properties"
        }
        ExpectedArena::Native("product_properties") => "native.iges.properties",
        ExpectedArena::Native(name) => panic!("matrix destination is not defined for {name}"),
    }
}

fn assert_matrix_destination(matrix: &toml::Value, subject_type: i64, arena: ExpectedArena) {
    let entity = matrix["entity"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["type"].as_integer() == Some(subject_type))
        .unwrap_or_else(|| panic!("entity type {subject_type} is absent from the envelope matrix"));
    let destination = entity["destination"].as_str().unwrap();
    let expected = matrix_destination(arena);
    assert!(
        destination.split(',').any(|item| item == expected),
        "entity type {subject_type} destination {destination:?} does not include {expected:?}"
    );
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
    assert_eq!(summary.format(), "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert!(summary.notes.iter().any(|note| note.contains("5.3")));
    decode_matrix(vec![
        (
            "nested_transformed_point",
            bytes,
            116,
            ExpectedArena::ModelPoints,
        ),
        ("point", point_file(), 116, ExpectedArena::ModelPoints),
        (
            "direction",
            direction_file(),
            123,
            ExpectedArena::Native("directions"),
        ),
        (
            "units_data",
            units_data_file(),
            316,
            ExpectedArena::Native("units_data"),
        ),
    ]);
}

#[test]
fn v4_outside_envelope_records_remain_native_without_neutral_projection() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let result = decode(owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 110,
                form: 1,
                label: "LATER-LN".into(),
                status: "00000000",
                parameters: "110,0,0,0,1,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "V4-POINT".into(),
                status: "00000000",
                parameters: "116,1,2,3,0;".into(),
            },
        ],
        global_v4,
    ));

    assert!(result.ir().model.curves.is_empty());
    assert_eq!(result.ir().model.points.len(), 1);
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas["entities"].len(), 2);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityOutsideEnvelope.kind()
            && loss.message
                == "IGES entity type 110 form 1 is outside the Fixed ASCII mechanical/document envelope"
    }));
}

#[test]
fn curve_pipeline_composes_analytic_conic_spline_composite_copious_and_offset_entities() {
    decode_matrix(vec![
        ("line", line_file(0), 110, ExpectedArena::ModelCurves),
        (
            "circular_arc",
            circular_arc_file(),
            100,
            ExpectedArena::ModelCurves,
        ),
        (
            "nurbs_curve",
            nurbs_curve_file(),
            126,
            ExpectedArena::ModelCurves,
        ),
        (
            "rational_nurbs_curve",
            rational_nurbs_curve_file(),
            126,
            ExpectedArena::ModelCurves,
        ),
        (
            "parametric_spline_curve",
            parametric_spline_curve_file(),
            112,
            ExpectedArena::ModelCurves,
        ),
        (
            "mixed_analytic_composite_curve",
            mixed_analytic_composite_curve_file(),
            102,
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "uniform_offset_circle",
            uniform_offset_circle_file(),
            130,
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "linear_offset_line",
            linear_offset_line_file(1),
            130,
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "function_offset_line",
            function_offset_line_file(),
            130,
            ExpectedArena::ModelProceduralCurves,
        ),
        (
            "copious_polyline",
            copious_data_file(11, b"106,1,2,0,0,0,1,0;", "00000000"),
            106,
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
            128,
            ExpectedArena::ModelSurfaces,
        ),
        (
            "parametric_spline_surface",
            parametric_spline_surface_file(),
            114,
            ExpectedArena::ModelSurfaces,
        ),
        (
            "ruled_surface",
            ruled_surface_file(),
            118,
            ExpectedArena::ModelProceduralSurfaces,
        ),
        (
            "tabulated_cylinder",
            tabulated_cylinder_file(),
            122,
            ExpectedArena::ModelProceduralSurfaces,
        ),
        (
            "surface_of_revolution",
            surface_of_revolution_file(),
            120,
            ExpectedArena::ModelProceduralSurfaces,
        ),
        (
            "placed_surface_of_revolution",
            placed_surface_of_revolution_file(),
            120,
            ExpectedArena::ModelProceduralSurfaces,
        ),
        (
            "offset_plane",
            offset_plane_file(1.0, 2.0),
            140,
            ExpectedArena::ModelProceduralSurfaces,
        ),
        (
            "parametrically_bounded_plane",
            parametrically_bounded_plane_file(),
            143,
            ExpectedArena::ModelRegions,
        ),
        (
            "trimmed_plane_with_inner_loop",
            trimmed_plane_with_inner_loop_file(),
            144,
            ExpectedArena::ModelRegions,
        ),
    ]);
}

#[test]
fn boundary_vertex_sewing_native_arena_preserves_source_coordinates() {
    let result = decode(bounded_plane_with_significance_gap_file());
    let records = &result
        .ir()
        .native
        .namespace("iges")
        .expect("IGES native namespace")
        .arenas["boundary_vertex_sewing"];

    assert!(records.iter().any(|record| {
        record.fields()["sewn"] == true
            && record.fields()["source_endpoints"]
                .as_array()
                .is_some_and(|endpoints| endpoints.len() > 1)
    }));
    let sewn = records
        .iter()
        .find(|record| record.fields()["sewn"] == true)
        .expect("a boundary coordinate gap is recorded as sewn");
    let fields = sewn.fields();
    assert_eq!(fields["source_entity"], "iges:entity:directory#13");
    assert_eq!(fields["tolerance"], 0.01);
    let endpoints = fields["source_endpoints"].as_array().unwrap();
    assert_eq!(endpoints.len(), 2);
    assert!(endpoints
        .iter()
        .any(|endpoint| endpoint["position"] == fields["representative"]));
}

#[test]
fn topology_pipeline_composes_manifold_nonmanifold_void_seam_and_boundary_graphs() {
    let (void_solid, _, _, _) = explicit_void_solid_file();
    decode_matrix(vec![
        (
            "explicit_tetrahedron_solid",
            explicit_tetrahedron_solid_file(),
            186,
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_open_shell",
            explicit_open_shell_file(),
            514,
            ExpectedArena::ModelShells,
        ),
        (
            "explicit_non_manifold_open_shell",
            explicit_non_manifold_open_shell_file(),
            514,
            ExpectedArena::ModelShells,
        ),
        (
            "explicit_vertex_loop",
            explicit_vertex_loop_file(),
            508,
            ExpectedArena::ModelLoops,
        ),
        (
            "explicit_void_solid",
            void_solid,
            186,
            ExpectedArena::ModelBodies,
        ),
        (
            "explicit_multi_pcurve_loop",
            explicit_multi_pcurve_loop_file(),
            508,
            ExpectedArena::ModelPcurves,
        ),
        (
            "explicit_cylinder_seam",
            explicit_cylinder_seam_file(),
            514,
            ExpectedArena::ModelShells,
        ),
        (
            "multi_pcurve_boundary",
            multi_pcurve_boundary_file(),
            141,
            ExpectedArena::ModelCoedges,
        ),
    ]);
}

#[test]
fn structure_pipeline_composes_csg_products_instances_patterns_groups_and_external_links() {
    decode_matrix(vec![
        (
            "primitive_solids",
            primitive_solids_file(),
            160,
            ExpectedArena::Native("primitive_solids"),
        ),
        (
            "procedural_and_boolean_solids",
            procedural_and_boolean_solids_file(),
            164,
            ExpectedArena::Native("procedural_solids"),
        ),
        (
            "solid_assembly",
            solid_assembly_file(),
            184,
            ExpectedArena::Native("solid_assemblies"),
        ),
        (
            "solid_instance",
            solid_instance_file(),
            430,
            ExpectedArena::Native("solid_instances"),
        ),
        (
            "patterned_instance",
            patterned_instance_file(),
            412,
            ExpectedArena::Native("rectangular_arrays"),
        ),
        (
            "external_reference_forms",
            external_reference_forms_file(),
            416,
            ExpectedArena::Native("external_references"),
        ),
        (
            "group_forms",
            group_forms_file(),
            402,
            ExpectedArena::Native("groups"),
        ),
        (
            "nested_subfigure",
            nested_subfigure_file(),
            308,
            ExpectedArena::Native("subfigure_definitions"),
        ),
        (
            "network_subfigure",
            network_subfigure_file(),
            320,
            ExpectedArena::Native("network_definitions"),
        ),
        (
            "connected_network_subfigure",
            connected_network_subfigure_file(),
            420,
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
            410,
            ExpectedArena::Native("views"),
        ),
        (
            "view_visibility_forms",
            view_visibility_forms_file(),
            402,
            ExpectedArena::Native("view_visibility"),
        ),
        (
            "segmented_view_visibility",
            segmented_view_visibility_file(),
            402,
            ExpectedArena::Native("segmented_visibility"),
        ),
        (
            "drawing_with_properties",
            drawing_with_properties_file(),
            404,
            ExpectedArena::Native("drawings"),
        ),
        (
            "text_annotation",
            text_annotation_file(),
            212,
            ExpectedArena::Native("annotations"),
        ),
        (
            "leader_forms",
            leader_forms_file(),
            214,
            ExpectedArena::Native("annotations"),
        ),
        (
            "dimension_forms",
            dimension_forms_file(),
            216,
            ExpectedArena::Native("annotations"),
        ),
        (
            "legacy_dimension_and_label_forms",
            legacy_dimension_and_label_forms_file(),
            202,
            ExpectedArena::Native("annotations"),
        ),
        (
            "symbol_and_sectioned_area",
            symbol_and_sectioned_area_file(),
            230,
            ExpectedArena::Native("annotations"),
        ),
        (
            "text_display_template_forms",
            text_display_template_forms_file(),
            312,
            ExpectedArena::Native("text_templates"),
        ),
        (
            "text_font_definition",
            text_font_definition_file(),
            310,
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
            322,
            ExpectedArena::Native("attribute_table_definitions"),
        ),
        (
            "attribute_instance_forms",
            attribute_instance_forms_file(),
            422,
            ExpectedArena::Native("attribute_table_instances"),
        ),
        (
            "product_property",
            product_property_file(),
            406,
            ExpectedArena::Native("product_properties"),
        ),
        (
            "scalar_property_forms",
            scalar_property_forms_file(),
            406,
            ExpectedArena::Native("properties"),
        ),
        (
            "grid_property",
            grid_property_file(),
            406,
            ExpectedArena::Native("properties"),
        ),
        (
            "dimension_property_forms",
            dimension_property_forms_file(),
            406,
            ExpectedArena::Native("properties"),
        ),
        (
            "drawing_metadata_property",
            drawing_metadata_property_forms_file(),
            406,
            ExpectedArena::Native("properties"),
        ),
        (
            "associativity_definition",
            associativity_definition_file(),
            302,
            ExpectedArena::Native("associativities"),
        ),
        (
            "bounded_associativity_forms",
            bounded_associativity_forms_file(),
            402,
            ExpectedArena::Native("associativities"),
        ),
        (
            "view_list_associativity",
            view_list_associativity_file(true),
            402,
            ExpectedArena::Native("associativities"),
        ),
        (
            "flow_associativity",
            flow_associativity_forms_file(),
            402,
            ExpectedArena::Native("associativities"),
        ),
        (
            "recalculable_dimension_associativity",
            recalculable_dimension_associativity_file(),
            402,
            ExpectedArena::Native("associativities"),
        ),
    ]);
}

#[test]
fn repeated_decode_is_canonical() {
    let bytes = explicit_tetrahedron_solid_with_boolean_file();
    let first = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let second = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        first.ir().to_canonical_json().unwrap(),
        second.ir().to_canonical_json().unwrap()
    );
    assert_eq!(
        serde_json::to_vec(first.report()).unwrap(),
        serde_json::to_vec(second.report()).unwrap()
    );
    assert_eq!(first.source_fidelity(), second.source_fidelity());
}

#[test]
fn cumulative_l8_domain_fixtures_validate_without_loss() {
    let (void_solid, _, _, _) = explicit_void_solid_file();
    let fixtures = [
        ("point", point_file()),
        (
            "conic",
            conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        ),
        ("nurbs-curve", rational_nurbs_curve_file()),
        ("spline-surface", parametric_spline_surface_file()),
        ("revolution", surface_of_revolution_file()),
        ("trimmed-sheet", trimmed_plane_with_inner_loop_file()),
        (
            "manifold-solid",
            explicit_tetrahedron_solid_with_boolean_file(),
        ),
        ("void-solid", void_solid),
        (
            "non-manifold-shell",
            explicit_non_manifold_open_shell_file(),
        ),
        ("appearance", colored_explicit_vertex_loop_file()),
        ("csg", primitive_solids_file()),
        ("solid-assembly", solid_assembly_file()),
        ("subfigures", nested_subfigure_file()),
        ("network", connected_network_subfigure_file()),
        ("external-references", external_reference_forms_file()),
        ("attribute-definitions", attribute_definition_forms_file()),
        ("attribute-instances", attribute_instance_forms_file()),
        ("properties", variable_schema_property_forms_file()),
        ("views", view_visibility_forms_file()),
        ("drawing", drawing_with_properties_file()),
        ("text", text_annotation_file()),
        ("symbols", symbol_and_sectioned_area_file()),
        ("associativity", bounded_associativity_forms_file()),
        ("text-font", text_font_definition_file()),
        ("units-data", units_data_file()),
    ];

    for (name, bytes) in fixtures {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bytes.as_slice()),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let loss_codes = result
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>();
        if name == "spline-surface" {
            assert_eq!(
                loss_codes,
                vec![IgesLossCode::SplineHeaderNotTransferred.kind()],
                "{name}: {:#?}",
                result.report().losses
            );
        } else {
            assert!(
                loss_codes.is_empty(),
                "{name}: {:#?}",
                result.report().losses
            );
        }
        let validation = cadmpeg_ir::validate_neutral_with_source_fidelity(
            result.ir(),
            result.source_fidelity(),
            Vec::new(),
        );
        assert!(validation.is_ok(), "{name}: {:#?}", validation.findings);
    }
}
