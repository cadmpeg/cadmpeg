// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized NX PRT byte images.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

use super::*;
use crate::test_support::*;

mod dialect;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized NX part should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir().native.namespace("nx").is_some());
}

#[test]
fn legacy_cfb_nx_detection_uses_ug_part_directory_evidence() {
    let bytes = legacy_cfb_with_ug_part();
    assert_eq!(NxCodec.detect(&bytes), Confidence::High);
    assert_eq!(NxCodec.detect(&bytes[..8]), Confidence::No);

    let summary = NxCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("legacy CFB NX inspection");
    assert_eq!(summary.container_kind, "cfb");
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("legacy CFB container")));
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == "parasolid-stream"));

    let result = decode(bytes);
    assert!(!result.report().geometry_transferred());
    assert!(!result.source_fidelity().retained_records.is_empty());
}

#[test]
fn legacy_cfb_nx_accepts_a_partial_final_stream_sector() {
    let bytes = legacy_cfb_with_partial_ug_part();
    let summary = NxCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("legacy CFB with a partial stream sector is inspectable");
    assert_eq!(summary.container_kind, "cfb");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == "parasolid-stream"));

    let result = decode(bytes);
    assert!(!result.report().geometry_transferred());
    assert!(!result.source_fidelity().retained_records.is_empty());
}

#[test]
fn legacy_cfb_catalogues_logical_stream_spans() {
    let summary = NxCodec
        .inspect(
            &mut Cursor::new(legacy_cfb_with_ug_part()),
            &InspectOptions::default(),
        )
        .expect("legacy CFB inspection");
    let part = summary
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
        .expect("legacy UG_PART stream entry");
    assert_eq!(part.role, "part-payload");
    assert!(part.compressed_size > 0);
    assert_eq!(part.compressed_size, part.uncompressed_size);
}

#[test]
fn legacy_cfb_catalogues_each_reachable_stream_in_a_disjoint_logical_span() {
    let bytes = legacy_cfb_with_two_streams();
    let summary = NxCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .expect("legacy CFB multi-stream inspection");
    let part = summary
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/UG_PART")
        .expect("legacy UG_PART stream entry");
    let extra = summary
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/UG_PART/Extra")
        .expect("legacy extra stream entry");
    assert_eq!(part.compressed_size, 10 * 512);
    assert_eq!(part.compressed_size, part.uncompressed_size);
    assert_eq!(extra.role, "named-opaque-stream");
    assert_eq!(extra.compressed_size, 8 * 512);
    assert_eq!(extra.compressed_size, extra.uncompressed_size);

    let result = decode(bytes);
    assert!(!result.source_fidelity().retained_records.is_empty());
}

#[test]
fn legacy_cfb_detection_rejects_the_compound_signature_without_ug_part_path() {
    let mut bytes = legacy_cfb_with_ug_part();
    let directory_entry = &mut bytes[512 + 2 * 128..512 + 3 * 128];
    for (offset, unit) in "OTHER".encode_utf16().enumerate() {
        put_u16(directory_entry, offset * 2, unit);
    }
    put_u16(directory_entry, 64, 12);

    assert_eq!(NxCodec.detect(&bytes), Confidence::No);
}

#[test]
fn splmsstr_pipeline_aligns_detection_inspection_and_parasolid_classification() {
    let bytes = single_part_prt();
    assert_eq!(NxCodec.detect(&bytes), Confidence::High);
    let summary = NxCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("NX inspection");
    assert_eq!(summary.format(), "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == "parasolid-stream"));
    assert!(summary.losses.iter().any(|loss| {
        loss.code == crate::loss::NxLossCode::KernelDialectUnverified.kind()
            && loss.message.contains("SCH_TEST_1_9999")
    }));

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert!(!result.source_fidelity().retained_records.is_empty());
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
        assert!(result.report().geometry_transferred());
        assert!(!result.ir().model.faces.is_empty());
        assert!(!result.ir().model.surfaces.is_empty());
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
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_)))
            || result
                .ir()
                .model
                .surfaces
                .iter()
                .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
        saw_pcurve |= !result.ir().model.pcurves.is_empty();
        saw_procedural_surface |= !result.ir().model.procedural_surfaces.is_empty();
        saw_procedural_curve |= !result.ir().model.procedural_curves.is_empty();
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
        assert!(result.report().geometry_transferred());
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 1);
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
        let namespace = result.ir().native.namespace("nx").unwrap();
        assert!(!namespace.arenas[expected_arena].is_empty());
        assert_valid(&result);
    }
}

#[test]
fn object_model_pipeline_projects_composed_feature_history_and_inputs() {
    let result = decode(composed_feature_history_prt());
    assert!(!result.ir().model.features.is_empty());
    let mut ordinals = result
        .ir()
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    assert_eq!(ordinals, (0..ordinals.len() as u64).collect::<Vec<_>>());
    let namespace = result.ir().native.namespace("nx").unwrap();
    assert!(!namespace.arenas["feature_operation_records"].is_empty());
    assert!(!namespace.arenas["feature_input_blocks"].is_empty());

    let labels = &namespace.arenas["feature_operation_labels"];
    let find_feature = |value: &str| {
        let label = labels
            .iter()
            .find(|label| {
                label
                    .field("value")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    == Some(value)
            })
            .expect("operation label");
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(label.id()))
            .expect("neutral feature")
    };
    let assert_native_links =
        |feature: &cadmpeg_ir::features::Feature, arena: &str, property_prefix: &str| {
            let operation_label = feature.native_ref.as_deref().expect("native feature label");
            let records = namespace.arenas[arena]
                .iter()
                .filter(|record| {
                    record
                        .field("operation_label")
                        .is_some_and(|value| value.as_str() == Some(operation_label))
                })
                .collect::<Vec<_>>();
            assert!(
                !records.is_empty(),
                "{arena} has no records for {operation_label}"
            );
            for (ordinal, record) in records.iter().enumerate() {
                assert_eq!(
                    feature
                        .source_properties
                        .get(&format!("{property_prefix}.{ordinal}"))
                        .map(String::as_str),
                    Some(record.id())
                );
            }
        };
    let csys = find_feature("DATUM_CSYS");
    assert_native_links(
        csys,
        "feature_datum_csys_payload_scalar_pairs",
        "datum_csys_payload_scalar_pair",
    );
    assert_native_links(
        csys,
        "feature_datum_csys_payload_fixed_pairs",
        "datum_csys_payload_fixed_pair",
    );
    assert_native_links(
        csys,
        "feature_datum_csys_payload_scalars",
        "datum_csys_payload_scalar",
    );
    assert_native_links(
        csys,
        "feature_datum_csys_descriptors",
        "datum_csys_descriptor",
    );
    let plane = find_feature("DATUM_PLANE");
    assert_native_links(
        plane,
        "feature_datum_plane_payload_scalar_pairs",
        "datum_plane_payload_scalar_pair",
    );
    assert_native_links(
        plane,
        "feature_datum_plane_descriptors",
        "datum_plane_descriptor",
    );
    assert_valid(&result);
}

#[test]
fn object_model_pipeline_projects_extract_body_source_from_offset_store() {
    let result = decode(extract_body_feature_history_prt());
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("EXTRACT_BODY"))
        .expect("EXTRACT_BODY feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::ExtractBody {
            source: cadmpeg_ir::features::BodySelection::Local { bodies, native },
        } if bodies.len() == 1
            && bodies[0].ends_with(":block#1")
            && native == "nx:om-object-index#1"
    ));
    assert!(feature.outputs.is_empty());
    assert_valid(&result);
}

#[test]
fn offset_store_primary_body_history_attaches_exact_writer_dependencies() {
    let result = decode(offset_store_primary_body_lineage_prt());
    let older = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000001")
        })
        .expect("older offset-store writer");
    let newer = result
        .ir()
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
    assert_eq!(
        older.source_properties.get("primary_body_reference"),
        Some(&String::from(
            "nx:feature-history:body-reference#0000000000-0000000001",
        )),
    );
    assert_eq!(
        older.source_properties.get("body_reference_occurrence.0"),
        Some(&String::from(
            "nx:feature-history:body-reference-occurrence#0000000000-0000000001-0000000000",
        )),
    );
    assert_eq!(
        newer.source_properties.get("primary_body_reference"),
        Some(&String::from(
            "nx:feature-history:body-reference#0000000000-0000000000",
        )),
    );
    assert_eq!(
        newer.source_properties.get("body_reference_occurrence.0"),
        Some(&String::from(
            "nx:feature-history:body-reference-occurrence#0000000000-0000000000-0000000000",
        )),
    );
    assert_eq!(result.ir().model.feature_result_topologies.len(), 2);
    assert_valid(&result);
}

#[test]
fn boolean_target_history_attaches_the_target_writer_dependency() {
    let result = decode(boolean_target_body_lineage_prt());
    let older = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000001")
        })
        .expect("Boolean target writer");
    let oldest = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000002")
        })
        .expect("oldest native body writer");
    let followup = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref()
                == Some("nx:feature-history:operation-label#0000000000-0000000000")
        })
        .expect("follow-up native body writer");
    assert!(oldest.dependencies.is_empty());
    assert_eq!(
        older.dependencies.as_slice(),
        std::slice::from_ref(&oldest.id)
    );
    assert_eq!(
        followup.dependencies.as_slice(),
        std::slice::from_ref(&older.id)
    );
    assert_eq!(result.ir().model.feature_result_topologies.len(), 3);
    assert_valid(&result);
}

#[test]
fn document_pipeline_retains_configurations_attributes_external_links_and_opaque_assets() {
    let document = decode(prt_with_arrangements());
    assert_eq!(document.ir().model.attributes.len(), 1);
    assert_eq!(document.ir().model.configurations.len(), 2);
    assert_valid(&document);

    let opaque = decode(prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/ExternalReferences", external_reference_stream()),
        ("/Root/vendor/private", b"opaque application state".to_vec()),
    ]));
    assert!(!opaque.ir().native_unknowns("nx").unwrap().is_empty());
    assert!(opaque.report().losses.iter().any(|loss| {
        loss.message.contains("ExternalReferences") || loss.message.contains("vendor/private")
    }));
    assert_valid(&opaque);
}

#[test]
fn container_identity_reaches_only_the_dialect_declaration() {
    let modern = decode(prt_with_indexed_om_section());
    let declared = &modern.report().dialects().unwrap().primary().declared();
    assert_eq!(declared["splmsstr_version"], "6");
    assert!(!declared.contains_key("ugii_version"));
    assert_eq!(declared.len(), 1);

    let legacy = decode(legacy_cfb_with_ug_part());
    let declared = &legacy.report().dialects().unwrap().primary().declared();
    assert!(declared.contains_key("ugii_version"));
    assert!(!declared.contains_key("splmsstr_version"));
    assert_eq!(declared.len(), 1);
}

#[test]
fn detect_high_on_magic() {
    assert_eq!(NxCodec.detect(MAGIC), Confidence::High);
    assert_eq!(NxCodec.detect(&single_part_prt()), Confidence::High);
    assert_eq!(NxCodec.detect(b"PK\x03\x04 not nx"), Confidence::No);
    // A Creo/Granite .prt shares the extension but not the magic.
    assert_eq!(NxCodec.detect(b"\xe0\x02\xff\xfeGRANITE"), Confidence::No);
}
