// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized NX PRT byte images.

use super::*;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized NX part should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir.native.namespace("nx").is_some());
}

#[test]
fn splmsstr_pipeline_aligns_detection_inspection_and_parasolid_classification() {
    let bytes = single_part_prt();
    assert_eq!(NxCodec.detect(&bytes), Confidence::High);
    let summary = NxCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("NX inspection");
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == "parasolid-stream"));
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("SCH_TEST_1_9999")));

    let result = decode(bytes);
    assert!(result.report.geometry_transferred);
    assert!(!result.source_fidelity.retained_records.is_empty());
    assert_valid(&result);
}

#[test]
fn analytic_topology_pipeline_covers_every_supported_quadric_and_conic_family() {
    let fixtures = [
        topology_partition_stream(),
        circle_topology_partition_stream(),
        ellipse_topology_partition_stream(),
        cylinder_topology_partition_stream(),
        cone_topology_partition_stream(),
        sphere_topology_partition_stream(),
        torus_topology_partition_stream(),
    ];
    for stream in fixtures {
        let result = decode(prt_with_partition(&stream));
        assert!(result.report.geometry_transferred);
        assert!(!result.ir.model.faces.is_empty());
        assert!(!result.ir.model.surfaces.is_empty());
        assert_valid(&result);
    }
}

#[test]
fn freeform_pipeline_binds_nurbs_pcurves_offsets_blends_and_intersections() {
    let fixtures = [
        bspline_partition_stream(),
        pcurve_topology_partition_stream(),
        offset_surface_topology_partition_stream(),
        blend_surface_topology_partition_stream(),
        charted_intersection_curve_topology_partition_stream(),
    ];
    let mut saw_nurbs = false;
    let mut saw_pcurve = false;
    let mut saw_procedural_surface = false;
    let mut saw_procedural_curve = false;
    for stream in fixtures {
        let result = decode(prt_with_partition(&stream));
        saw_nurbs |= result
            .ir
            .model
            .curves
            .iter()
            .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_)))
            || result
                .ir
                .model
                .surfaces
                .iter()
                .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
        saw_pcurve |= !result.ir.model.pcurves.is_empty();
        saw_procedural_surface |= !result.ir.model.procedural_surfaces.is_empty();
        saw_procedural_curve |= !result.ir.model.procedural_curves.is_empty();
        assert_valid(&result);
    }
    assert!(saw_nurbs);
    assert!(saw_pcurve);
    assert!(saw_procedural_surface);
    assert!(saw_procedural_curve);
}

#[test]
fn deltas_pipeline_replaces_geometry_without_discarding_partition_topology() {
    let partition = topology_partition_stream();
    let delta_fixtures = [
        deltas_point_partition_stream(),
        deltas_edge_partition_stream(),
        deltas_face_vertex_partition_stream(),
        deltas_loop_partition_stream(),
        deltas_shell_partition_stream(),
        deltas_fin_partition_stream(),
        deltas_line_partition_stream(),
        deltas_plane_partition_stream(),
    ];
    for deltas in delta_fixtures {
        let result = decode(prt_with_streams(&[&partition, &deltas]));
        assert!(result.report.geometry_transferred);
        assert_eq!(result.ir.model.bodies.len(), 1);
        assert_eq!(result.ir.model.faces.len(), 1);
        assert_valid(&result);
    }
}

#[test]
fn display_jt_pipeline_decodes_mesh_scene_lod_and_property_streams() {
    let fixtures = [
        (display_jt_basic_stream(), "display_jt_documents"),
        (
            display_jt_scene_graph_stream(),
            "display_jt_group_node_data",
        ),
        (
            display_jt_shape_lod_stream(),
            "display_jt_shape_lod_elements",
        ),
        (
            display_jt_string_property_stream(),
            "display_jt_string_property_atoms",
        ),
    ];
    for (jt, expected_arena) in fixtures {
        let result = decode(prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/UG_PART/DisplayJT", jt),
        ]));
        let namespace = result.ir.native.namespace("nx").unwrap();
        assert!(!namespace.arenas[expected_arena].is_empty());
        assert_valid(&result);
    }
}

#[test]
fn object_model_pipeline_projects_composed_feature_history_and_inputs() {
    let result = decode(composed_feature_history_prt());
    assert!(!result.ir.model.features.is_empty());
    let mut ordinals = result
        .ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    assert_eq!(ordinals, (0..ordinals.len() as u64).collect::<Vec<_>>());
    let namespace = result.ir.native.namespace("nx").unwrap();
    assert!(!namespace.arenas["feature_operation_records"].is_empty());
    assert!(!namespace.arenas["feature_input_blocks"].is_empty());
    assert_valid(&result);
}

#[test]
fn offset_store_primary_body_history_attaches_exact_writer_dependencies() {
    let result = decode(offset_store_primary_body_lineage_prt());
    let older = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000001")
        })
        .expect("older offset-store writer");
    let newer = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000000")
        })
        .expect("newer offset-store writer");

    assert!(older.dependencies.is_empty());
    assert_eq!(
        newer.dependencies.as_slice(),
        std::slice::from_ref(&older.id)
    );
    assert_eq!(result.ir.model.feature_result_topologies.len(), 2);
    assert_valid(&result);
}

#[test]
fn boolean_target_history_attaches_the_target_writer_dependency() {
    let result = decode(boolean_target_body_lineage_prt());
    let older = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000001")
        })
        .expect("older native body writer");
    let newer = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000000")
        })
        .expect("Boolean target writer");
    assert!(older.dependencies.is_empty());
    assert_eq!(
        newer.dependencies.as_slice(),
        std::slice::from_ref(&older.id)
    );
    assert_eq!(result.ir.model.feature_result_topologies.len(), 2);
    assert_valid(&result);
}

#[test]
fn document_pipeline_retains_configurations_attributes_external_links_and_opaque_assets() {
    let document = decode(prt_with_arrangements());
    assert_eq!(document.ir.model.attributes.len(), 1);
    assert_eq!(document.ir.model.configurations.len(), 2);
    assert_valid(&document);

    let opaque = decode(prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/ExternalReferences", external_reference_stream()),
        ("/Root/vendor/private", b"opaque application state".to_vec()),
    ]));
    assert!(!opaque.ir.native_unknowns("nx").unwrap().is_empty());
    assert!(opaque.report.losses.iter().any(|loss| {
        loss.message.contains("ExternalReferences") || loss.message.contains("vendor/private")
    }));
    assert_valid(&opaque);
}
