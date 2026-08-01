// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized OpenNURBS archives.

use super::*;
use crate::archive_test_support as support;
use crate::{RhinoArchiveVersion, RhinoEncoder};
use cadmpeg_ir::Encoder;

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized 3DM archive should decode")
}

fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir.native.namespace("rhino").is_some());
}

#[test]
fn archive_pipeline_aligns_versions_detection_inspection_units_and_container_only_modes() {
    for version in ["50", "60", "70", "80"] {
        let object = support::object_record(
            1,
            support::POINT_CLASS,
            &support::point_payload([1.0, 2.0, 3.0]),
        );
        let bytes = support::archive_version(version, &[object]);
        assert_eq!(RhinoCodec.detect(&bytes), Confidence::High);
        let summary = RhinoCodec
            .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
            .expect("3DM inspection");
        assert_eq!(summary.format, "rhino");
        assert!(summary.notes.iter().any(|note| note.contains(version)));
        let result = decode(bytes.clone());
        assert_eq!(result.ir.model.points.len(), 1);
        assert_valid(&result);

        let container = RhinoCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .expect("container-only 3DM decode");
        assert!(container.report.container_only);
        assert!(container.ir.model.points.is_empty());
    }
}

#[test]
fn curve_pipeline_composes_points_clouds_lines_arcs_polylines_and_compounds() {
    let children = [
        (
            support::LINE_CLASS,
            support::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        ),
        (
            support::ARC_CLASS,
            support::arc_payload([0.0, 1.0], [1.0, 2.0]),
        ),
    ];
    let objects = vec![
        support::object_record(
            1,
            support::POINT_CLASS,
            &support::point_payload([1.0, 2.0, 3.0]),
        ),
        support::object_record(
            2,
            support::POINT_CLOUD_CLASS,
            &support::point_cloud_payload(&[[0.0, 0.0, 0.0], [2.0, 3.0, 4.0]]),
        ),
        support::object_record(
            4,
            support::LINE_CLASS,
            &support::line_payload([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [-1.0, 3.0]),
        ),
        support::object_record(
            4,
            support::ARC_CLASS,
            &support::arc_payload([0.0, 1.0], [0.0, 1.0]),
        ),
        support::object_record(
            4,
            support::POLYLINE_CLASS,
            &support::polyline_payload(
                &[[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
                &[2.0, 3.5, 9.0],
            ),
        ),
        support::object_record(
            4,
            support::POLYCURVE_CLASS,
            &support::polycurve_payload(&[0.0, 2.0, 5.0], &children),
        ),
    ];
    let result = decode(support::archive(&objects));
    assert!(!result.ir.model.points.is_empty());
    assert!(result.ir.model.curves.len() >= 3);
    assert!(!result.ir.model.procedural_curves.is_empty());
    assert_valid(&result);
}

#[test]
fn geometry_pipeline_composes_mesh_subd_extrusion_and_connected_brep_objects() {
    let objects = vec![
        support::object_record(
            0x20,
            support::MESH_CLASS,
            &support::mesh_payload(3, 5, false, true),
        ),
        support::object_record(
            0x30,
            support::SUBD_CLASS,
            &crate::subd::tests::quad_payload(ArchiveVersion::V8),
        ),
        support::object_record(
            0x10,
            support::EXTRUSION_CLASS,
            &crate::extrusion::tests::archive_payload(3, [true, false], false, true),
        ),
        support::object_record(0x10, support::BREP_CLASS, &support::brep_payload(false)),
    ];
    let result = decode(support::archive_writer("80", 202_608_010, &objects));
    assert!(!result.ir.model.tessellations.is_empty());
    assert!(!result.ir.model.subds.is_empty());
    assert!(!result.ir.model.procedural_surfaces.is_empty());
    assert!(!result.ir.model.faces.is_empty());
    assert!(!result.ir.model.pcurves.is_empty());
    assert_valid(&result);
}

#[test]
fn instance_pipeline_composes_nested_transforms_membership_and_analytic_conversion() {
    static_instance_suppresses_member_and_two_references_expand_with_distinct_ids();
    nested_instance_composes_parent_child_and_records_outer_to_inner_path();
    nil_and_duplicate_reference_ids_use_distinct_record_path_segments();
    instance_bakes_mesh_subd_and_normals_without_changing_subd_metadata();
    nonuniform_instance_converts_analytic_circle_to_exact_nurbs();
    transformed_procedural_instance_keeps_solved_carriers_without_dangling_references();
}

#[test]
fn document_pipeline_composes_definitions_history_identity_attributes_and_settings() {
    parses_source_shaped_v5_minor_6_and_7_definition_records();
    parses_source_shaped_v6_v7_v8_static_and_linked_definitions();
    scan_decodes_history_identity_dependencies_and_typed_values();
    identity_resolution_defers_material_and_parent_colors();
    scans_metadata_tables_and_reports_offsets();
    parses_units_with_single_scale_transfer_and_legacy_order();
}

#[test]
fn writer_pipeline_round_trips_supported_versions_and_connected_source_less_topology() {
    let mut point_ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    point_ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("integration:point#0".into()),
        position: cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75),
        source_object: None,
    });
    for version in [
        crate::RhinoArchiveVersion::V5,
        crate::RhinoArchiveVersion::V6,
        crate::RhinoArchiveVersion::V7,
        crate::RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        crate::RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &point_ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .unwrap();
        let result = decode(bytes);
        assert_eq!(
            result.ir.model.points[0].position,
            point_ir.model.points[0].position
        );
        assert_valid(&result);
    }

    let mut sheet = decode(support::archive(&[support::object_record(
        0x10,
        support::BREP_CLASS,
        &support::brep_payload(false),
    )]))
    .ir;
    sheet.source = None;
    sheet.native.0.remove("rhino");
    for curve in &mut sheet.model.curves {
        curve.source_object = None;
    }
    for surface in &mut sheet.model.surfaces {
        surface.source_object = None;
    }
    for point in &mut sheet.model.points {
        point.source_object = None;
    }
    let mut bytes = Vec::new();
    RhinoEncoder::new(RhinoArchiveVersion::V8)
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &sheet,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    let result = decode(bytes);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_valid(&result);
}

#[test]
fn recovery_pipeline_keeps_malformed_records_atomic_and_later_objects_decodable() {
    for malformed in [
        support::object_record(1, support::POINT_CLASS, &[0x20]),
        support::object_record(
            1,
            support::POINT_CLASS,
            &support::point_payload([f64::NAN, 0.0, 0.0]),
        ),
    ] {
        let valid = support::object_record(
            1,
            support::POINT_CLASS,
            &support::point_payload([4.0, 5.0, 6.0]),
        );
        let result = decode(support::archive(&[malformed, valid]));
        assert_eq!(result.ir.model.points.len(), 1);
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.severity >= Severity::Warning));
        assert_valid(&result);
    }
    attribute_userdata_recovers_after_malformed_bounded_record();
    malformed_bounded_object_is_retained_and_later_point_decodes();
    structural_framing_errors_keep_diagnostics();
}
