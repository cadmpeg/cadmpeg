// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! End-to-end contracts over synthesized `OpenNURBS` archives.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind;
use cadmpeg_ir::Encoder;

use crate::chunks::ArchiveVersion;
use crate::test_support as support;
use crate::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};
mod angular_dimension_userdata;
mod annotations_userdata;
mod dimension_userdata;
mod hatch_userdata;
mod layer_userdata;
mod mesh_modifiers_userdata;
mod mesh_userdata;
mod object_attributes_userdata;
mod settings_userdata;
mod unknown_userdata;
mod v5_hatch_extra_userdata;
mod views_userdata;
fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized 3DM archive should decode")
}
fn assert_valid(result: &cadmpeg_ir::codec::DecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(result.ir().native.namespace("rhino").is_some());
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
        assert_eq!(result.ir().model.points.len(), 1);
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
        assert!(container.report().container_only);
        assert!(container.ir().model.points.is_empty());
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
    assert!(!result.ir().model.points.is_empty());
    assert!(result.ir().model.curves.len() >= 3);
    assert!(!result.ir().model.procedural_curves.is_empty());
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
    assert!(!result.ir().model.tessellations.is_empty());
    assert!(!result.ir().model.subds.is_empty());
    assert!(!result.ir().model.procedural_surfaces.is_empty());
    assert!(!result.ir().model.faces.is_empty());
    assert!(!result.ir().model.pcurves.is_empty());
    assert_valid(&result);
}

#[test]
fn instance_pipeline_composes_nested_transforms_membership_and_analytic_conversion() {
    crate::instances::tests::static_instance_suppresses_member_and_two_references_expand_with_distinct_ids();
    crate::instances::tests::nested_instance_composes_parent_child_and_records_outer_to_inner_path(
    );
    crate::instances::tests::nil_and_duplicate_reference_ids_use_distinct_record_path_segments();
    crate::instances::tests::instance_bakes_mesh_subd_and_normals_without_changing_subd_metadata();
    crate::instances::tests::nonuniform_instance_converts_analytic_circle_to_exact_nurbs();
    crate::instances::tests::transformed_procedural_instance_keeps_solved_carriers_without_dangling_references();
}

#[test]
fn document_pipeline_composes_definitions_history_identity_attributes_and_settings() {
    crate::instances::tests::parses_source_shaped_v5_minor_6_and_7_definition_records();
    crate::instances::tests::parses_source_shaped_v6_v7_v8_static_and_linked_definitions();
    crate::history::tests::scan_decodes_history_identity_dependencies_and_typed_values();
    crate::objects::tests::identity_resolution_defers_material_and_parent_colors();
    crate::container::tests::scans_metadata_tables_and_reports_offsets();
    crate::settings::tests::parses_units_with_single_scale_transfer_and_legacy_order();
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
            result.ir().model.points[0].position,
            point_ir.model.points[0].position
        );
        assert_valid(&result);
    }

    let (mut sheet, _, _) = decode(support::archive(&[support::object_record(
        0x10,
        support::BREP_CLASS,
        &support::brep_payload(false),
    )]))
    .into_parts();
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
    assert_eq!(result.ir().model.faces.len(), 1);
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
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity >= Severity::Warning));
        assert_valid(&result);
    }
    crate::objects::tests::attribute_userdata_recovers_after_malformed_bounded_record();
    crate::objects::tests::malformed_bounded_object_is_retained_and_later_point_decodes();
    crate::container::tests::structural_framing_errors_keep_diagnostics();
}

#[test]
fn registered_future_object_major_is_retained_without_known_prefix() {
    let mut future_point = support::point_payload([1.0, 2.0, 3.0]);
    future_point[0] = 0x20;
    future_point.extend([0xde, 0xad]);
    let future_record = support::object_record(1, support::POINT_CLASS, &future_point);
    let valid_record = support::object_record(
        1,
        support::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let result = decode(support::archive(&[future_record.clone(), valid_record]));

    assert_eq!(result.ir().model.points.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id == "rhino:object:record#000000")
        .expect("future-major object record is retained");
    assert_eq!(retained.data.as_deref(), Some(future_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("simple geometry retained")
            && loss.message.contains("unsupported version")
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ObjectRecordCensus.kind()
            && loss.message.contains("decoded 1/2 Rhino object records")
    }));
    assert_valid(&result);
}

#[test]
fn registered_future_table_major_is_retained_without_known_prefix() {
    let archive = ArchiveVersion::V5;
    let group_class = crate::wire::Uuid::from_canonical([
        0x72, 0x1d, 0x9f, 0x97, 0x36, 0x45, 0x44, 0xc4, 0x8b, 0xe6, 0xb2, 0xcf, 0x69, 0x7d, 0x25,
        0xce,
    ])
    .to_wire();
    let mut future_group_payload = vec![0x21];
    future_group_payload.extend(7_i32.to_le_bytes());
    future_group_payload.extend(crate::test_support::test_dump::utf16_bytes("future group"));
    future_group_payload.extend([0_u8; 16]);
    future_group_payload.extend([0xde, 0xad]);
    let future_group = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8073,
        &crate::test_support::test_dump::class_wrapper(archive, group_class, &future_group_payload),
    );
    let valid_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0018,
                std::slice::from_ref(&future_group),
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000018-20008073-")
        })
        .expect("future-major table record is retained");
    assert_eq!(retained.data.as_deref(), Some(future_group.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
            && loss.message.contains("could not be transferred")
    }));
    assert_valid(&result);
}
#[test]
fn registered_userdata_future_payload_is_retained_by_table_owner() {
    let archive = ArchiveVersion::V5;
    let light_class = crate::wire::Uuid::from_canonical([
        0x85, 0xa0, 0x85, 0x13, 0xf3, 0x83, 0x11, 0xd3, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    let mut light_payload = vec![0x1f];
    light_payload.extend(1_i32.to_le_bytes());
    light_payload.extend(4_i32.to_le_bytes());
    light_payload.extend(0.5_f64.to_le_bytes());
    light_payload.extend(20.0_f64.to_le_bytes());
    light_payload.extend([1, 2, 3, 4]);
    light_payload.extend([5, 6, 7, 8]);
    light_payload.extend([9, 10, 11, 12]);
    for value in [0.0_f64, 0.0, -1.0, 1.0, 2.0, 3.0] {
        light_payload.extend(value.to_le_bytes());
    }
    light_payload.extend(0.25_f64.to_le_bytes());
    light_payload.extend(16.0_f64.to_le_bytes());
    for value in [1.0_f64, 0.0, 0.0] {
        light_payload.extend(value.to_le_bytes());
    }
    light_payload.extend(0.75_f64.to_le_bytes());
    light_payload.extend(3_i32.to_le_bytes());
    light_payload.extend([0x55; 16]);
    light_payload.extend(crate::test_support::test_dump::utf16_bytes("key"));
    for value in [4.0_f64, 0.0, 0.0, 0.0, 5.0, 0.0] {
        light_payload.extend(value.to_le_bytes());
    }
    light_payload.extend(0.8_f64.to_le_bytes());
    light_payload.extend([0xaa, 0xbb]);

    let mut light_body =
        crate::test_support::test_dump::class_wrapper(archive, light_class, &light_payload);
    let mut attributes = vec![0x20];
    attributes.extend([0; 16]);
    attributes.extend(7_i32.to_le_bytes());
    attributes.push(1);
    attributes.extend(crate::test_support::test_dump::utf16_bytes("table light"));
    attributes.extend([11, 0, 0]);
    light_body.extend(crate::test_support::test_dump::crc_chunk(
        archive,
        0x0200_8061,
        &attributes,
    ));

    let future_list_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::USER_STRING_LIST.to_wire(),
        crate::wire::Uuid::from_canonical([
            0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe,
            0x52, 0x0d,
        ])
        .to_wire(),
        50,
        0,
        &future_list_payload,
    );
    let attribute_userdata = [
        userdata,
        crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0),
    ]
    .concat();
    light_body.extend(crate::test_support::test_dump::long_chunk(
        archive,
        0x0200_0062,
        &attribute_userdata,
    ));
    light_body.extend(crate::test_support::test_dump::short_chunk(
        archive,
        0x8200_006f,
        0,
    ));
    let light_record =
        crate::test_support::test_dump::nested_crc_chunk(archive, 0x2000_8060, &light_body);
    let valid_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0012,
                std::slice::from_ref(&light_record),
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 1);
    let lights = &result.ir().native.namespace("rhino").unwrap().arenas["lights"];
    assert_eq!(lights.len(), 1);
    assert_eq!(
        lights[0]
            .field("name")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("key".to_owned())
    );
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000012-20008060-")
        })
        .expect("future userdata table record is retained");
    assert_eq!(retained.data.as_deref(), Some(light_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("user-string") && loss.message.contains("unsupported")
    }));
    assert_valid(&result);
}

#[test]
fn registered_material_userdata_future_payload_is_retained_by_table_owner() {
    let archive = ArchiveVersion::V5;
    let material_class = crate::wire::Uuid::from_canonical([
        0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    let mut material_payload = [[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
    material_payload.extend(crate::test_support::test_dump::utf16_bytes("steel"));
    material_payload.extend([0x22; 16]);
    for color in [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
        [128, 128, 128, 24],
    ] {
        material_payload.extend(color);
    }
    for value in [1.5_f64, 0.25, 64.0, 0.1] {
        material_payload.extend(value.to_le_bytes());
    }
    material_payload.extend(crate::test_support::test_dump::anonymous_chunk(
        archive,
        0,
        &0_i32.to_le_bytes(),
    ));
    material_payload.extend(crate::test_support::test_dump::utf16_bytes(""));
    material_payload.extend(0_i32.to_le_bytes());
    material_payload.extend([1, 0]);
    material_payload.push(1);
    for value in [0.9_f64, 0.8, 1.4] {
        material_payload.extend(value.to_le_bytes());
    }
    material_payload.extend([0x33; 16]);
    material_payload.push(1);
    material_payload.extend([0xaa, 0xbb]);
    let mut material_class_data = vec![0x20];
    material_class_data.extend(crate::test_support::test_dump::anonymous_chunk(
        archive,
        7,
        &material_payload,
    ));

    let future_pbr_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            &[0xde, 0xad],
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::wire::Uuid::from_canonical([
            0x56, 0x94, 0xe1, 0xac, 0x40, 0xe6, 0x44, 0xf4, 0x9c, 0xa9, 0x3b, 0x6d, 0x0e, 0x8c,
            0x44, 0x40,
        ])
        .to_wire(),
        crate::wire::Uuid::from_canonical([
            0x7b, 0x0b, 0x58, 0x5d, 0x7a, 0x31, 0x45, 0xd0, 0x92, 0x5e, 0xbd, 0xd7, 0xdd, 0xf3,
            0xe4, 0xe3,
        ])
        .to_wire(),
        50,
        0,
        &future_pbr_payload,
    );
    let mut uuid_body = material_class.to_vec();
    uuid_body.extend(crc32fast::hash(&material_class).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data =
        crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffc, &material_class_data);
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let material_class_wrapper = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let material_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8040,
        &material_class_wrapper,
    );
    let valid_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0010,
                std::slice::from_ref(&material_record),
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 1);
    let materials = &result.ir().native.namespace("rhino").unwrap().arenas["materials"];
    assert_eq!(materials.len(), 1);
    assert_eq!(
        materials[0]
            .field("name")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("steel".to_owned())
    );
    assert!(materials[0].field("physically_based").is_none());
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000010-20008040-")
        })
        .expect("future material userdata record is retained");
    assert_eq!(retained.data.as_deref(), Some(material_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("physically based material userdata")
            && loss.message.contains("could not be transferred")
    }));
    assert_valid(&result);
}

#[test]
fn registered_dimension_style_userdata_future_payload_is_retained_by_table_owner() {
    let archive = ArchiveVersion::V5;
    let v5_dimstyle_class = crate::wire::Uuid::from_canonical([
        0x81, 0xbd, 0x83, 0xd5, 0x71, 0x20, 0x41, 0xc4, 0x9a, 0x57, 0xc4, 0x49, 0x33, 0x6f, 0xf1,
        0x2c,
    ])
    .to_wire();
    let mut dimstyle_payload = vec![0x15];
    dimstyle_payload.extend(7_i32.to_le_bytes());
    dimstyle_payload.extend(crate::test_support::test_dump::utf16_bytes(
        "legacy dimension style",
    ));
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0] {
        dimstyle_payload.extend(value.to_le_bytes());
    }
    dimstyle_payload.extend(6_u32.to_le_bytes());
    dimstyle_payload.extend(7_i32.to_le_bytes());
    dimstyle_payload.extend(8_i32.to_le_bytes());
    dimstyle_payload.extend(9_u32.to_le_bytes());
    dimstyle_payload.extend(10_u32.to_le_bytes());
    dimstyle_payload.extend(11_i32.to_le_bytes());
    dimstyle_payload.extend(12_i32.to_le_bytes());
    dimstyle_payload.extend(13_i32.to_le_bytes());
    dimstyle_payload.extend(14.0_f64.to_le_bytes());
    dimstyle_payload.extend(15.0_f64.to_le_bytes());
    dimstyle_payload.extend(crate::test_support::test_dump::utf16_bytes("<"));
    dimstyle_payload.extend(crate::test_support::test_dump::utf16_bytes(">"));
    dimstyle_payload.push(1);
    dimstyle_payload.extend(16.0_f64.to_le_bytes());
    dimstyle_payload.extend(17_u32.to_le_bytes());
    dimstyle_payload.extend(18_i32.to_le_bytes());
    dimstyle_payload.extend(19_u32.to_le_bytes());
    dimstyle_payload.extend(20_i32.to_le_bytes());
    dimstyle_payload.extend(crate::test_support::test_dump::utf16_bytes("["));
    dimstyle_payload.extend(crate::test_support::test_dump::utf16_bytes("]"));
    dimstyle_payload.extend(21_u32.to_le_bytes());
    dimstyle_payload.extend([0x33; 16]);
    dimstyle_payload.extend(22.0_f64.to_le_bytes());
    dimstyle_payload.extend(23.0_f64.to_le_bytes());
    dimstyle_payload.extend(24_i32.to_le_bytes());
    dimstyle_payload.extend([1, 0]);

    let future_extra_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
            &[0xbe, 0xef],
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::wire::Uuid::from_canonical([
            0x51, 0x3f, 0xde, 0x53, 0x72, 0x84, 0x40, 0x65, 0x86, 0x01, 0x06, 0xce, 0xa8, 0xb2,
            0x8d, 0x6f,
        ])
        .to_wire(),
        crate::wire::Uuid::from_canonical([
            0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc,
            0x30, 0xd4,
        ])
        .to_wire(),
        50,
        0,
        &future_extra_payload,
    );
    let mut uuid_body = v5_dimstyle_class.to_vec();
    uuid_body.extend(crc32fast::hash(&v5_dimstyle_class).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data =
        crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffc, &dimstyle_payload);
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let dimstyle_class_wrapper = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let dimstyle_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8075,
        &dimstyle_class_wrapper,
    );
    let valid_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0020,
                std::slice::from_ref(&dimstyle_record),
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 1);
    let dimension_styles =
        &result.ir().native.namespace("rhino").unwrap().arenas["dimension_styles"];
    assert_eq!(dimension_styles.len(), 1);
    assert_eq!(
        dimension_styles[0]
            .field("name")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("legacy dimension style".to_owned())
    );
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000020-20008075-")
        })
        .expect("future dimension-style userdata record is retained");
    assert_eq!(retained.data.as_deref(), Some(dimstyle_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("V5 dimension-style userdata")
            && loss.message.contains("could not be transferred")
    }));
    assert_valid(&result);
}

#[test]
fn material_rdk_userdata_is_retained_as_callback_owned_source() {
    let archive = ArchiveVersion::V5;
    let material_class = crate::wire::Uuid::from_canonical([
        0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ])
    .to_wire();
    let mut material_payload = [[0x11; 16].as_slice(), 7_i32.to_le_bytes().as_slice()].concat();
    material_payload.extend(crate::test_support::test_dump::utf16_bytes("rdk material"));
    material_payload.extend([0x22; 16]);
    for color in [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
        [21, 22, 23, 24],
    ] {
        material_payload.extend(color);
    }
    for value in [1.5_f64, 0.25, 64.0, 0.1] {
        material_payload.extend(value.to_le_bytes());
    }
    material_payload.extend(crate::test_support::test_dump::anonymous_chunk(
        archive,
        0,
        &0_i32.to_le_bytes(),
    ));
    let mut material_class_data = vec![0x20];
    material_class_data.extend(crate::test_support::test_dump::anonymous_chunk(
        archive,
        0,
        &material_payload,
    ));

    let xml = b"<xml><render-content-manager-data><material instance-id=\"44444444-4444-4444-4444-444444444444\"/></render-content-manager-data></xml>\0";
    let rdk_payload = [
        2_i32.to_le_bytes().as_slice(),
        (xml.len() as i32).to_le_bytes().as_slice(),
        xml.as_slice(),
    ]
    .concat();
    let userdata =
        crate::test_support::test_dump::class_userdata_v2_with_class_and_item_direct_payload(
            archive,
            crate::wire::Uuid::from_canonical([
                0xaf, 0xa8, 0x27, 0x72, 0x15, 0x25, 0x43, 0xdd, 0xa6, 0x3c, 0xc8, 0x4a, 0xc5, 0x80,
                0x69, 0x11,
            ])
            .to_wire(),
            crate::wire::Uuid::from_canonical([
                0xb6, 0x3e, 0xd0, 0x79, 0xcf, 0x67, 0x41, 0x6c, 0x80, 0x0d, 0x22, 0x02, 0x3a, 0xe1,
                0xbe, 0x21,
            ])
            .to_wire(),
            crate::wire::Uuid::from_canonical([
                0x16, 0x59, 0x2d, 0x58, 0x4a, 0x2f, 0x40, 0x1d, 0xbf, 0x5e, 0x3b, 0x87, 0x74, 0x1c,
                0x1b, 0x1b,
            ])
            .to_wire(),
            50,
            0,
            &rdk_payload,
        );
    let mut uuid_body = material_class.to_vec();
    uuid_body.extend(crc32fast::hash(&material_class).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data =
        crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffc, &material_class_data);
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let material_class_wrapper = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let material_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8040,
        &material_class_wrapper,
    );
    let valid_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0010,
                std::slice::from_ref(&material_record),
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 1);
    let materials = &result.ir().native.namespace("rhino").unwrap().arenas["materials"];
    assert_eq!(materials.len(), 1);
    assert_eq!(
        materials[0]
            .field("name")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("rdk material".to_owned())
    );
    assert!(materials[0]
        .field("rdk_instance_uuid")
        .and_then(|value| value.as_str().map(str::to_owned))
        .is_none());
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000010-20008040-")
        })
        .expect("callback-owned RDK material record is retained");
    assert_eq!(retained.data.as_deref(), Some(material_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("RDK material userdata")
            && loss.message.contains("could not be transferred")
    }));
    assert_valid(&result);
}

#[test]
fn object_user_string_userdata_future_payload_is_retained_with_typed_geometry() {
    let archive = ArchiveVersion::V5;
    let future_list_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::USER_STRING_LIST.to_wire(),
        crate::wire::Uuid::from_canonical([
            0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe,
            0x52, 0x0d,
        ])
        .to_wire(),
        50,
        0,
        &future_list_payload,
    );
    let object_type = crate::test_support::test_dump::short_chunk(archive, 0x8200_0071, 1);
    let mut uuid_body = crate::test_support::test_dump::POINT_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&crate::test_support::test_dump::POINT_CLASS).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crate::test_support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &support::point_payload([1.0, 2.0, 3.0]),
    );
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let attributes = crate::test_support::test_dump::crc_chunk(
        archive,
        0x0200_8072,
        &crate::test_support::test_dump::fixed_attributes(0, 0, Some(true)),
    );
    let object_end = crate::test_support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    let object_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | crate::chunks::TCODE_CRC,
        &[object_type, class, attributes, object_end].concat(),
    );
    let following_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0013,
                &[object_record.clone(), following_point],
            ),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 2);
    let presentation =
        &result.ir().native.namespace("rhino").unwrap().arenas["object_presentation"];
    assert_eq!(presentation.len(), 1);
    assert_ne!(
        presentation[0]
            .field("user_strings")
            .and_then(|value| value.as_array().map(|values| !values.is_empty())),
        Some(true)
    );
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id == "rhino:object:record#000000")
        .expect("future object userdata record is retained");
    assert_eq!(retained.data.as_deref(), Some(object_record.as_slice()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("object user-string userdata") && loss.message.contains("unsupported")
    }));
    assert_valid(&result);
}

#[test]
fn mesh_subd_proxy_future_payload_retains_parent_mesh_record() {
    let archive = ArchiveVersion::V5;
    let future_proxy_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::subd::SUBD_MESH_PROXY_USERDATA.to_wire(),
        crate::wire::Uuid::from_canonical([
            0x7b, 0x0b, 0x58, 0x5d, 0x7a, 0x31, 0x45, 0xd0, 0x92, 0x5e, 0xbd, 0xd7, 0xdd, 0xf3,
            0xe4, 0xe3,
        ])
        .to_wire(),
        50,
        0,
        &future_proxy_payload,
    );
    let object_type = crate::test_support::test_dump::short_chunk(archive, 0x8200_0071, 0x20);
    let mut uuid_body = crate::test_support::test_dump::MESH_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&crate::test_support::test_dump::MESH_CLASS).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crate::test_support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &support::mesh_payload(3, 5, false, true),
    );
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let object_end = crate::test_support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    let mesh_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | crate::chunks::TCODE_CRC,
        &[object_type, class, object_end].concat(),
    );
    let following_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let bytes = support::archive_writer("50", 202_608_010, &[mesh_record.clone(), following_point]);
    let result = decode(bytes);

    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(result.ir().model.subds.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("SubD mesh proxy dropped")
            && loss.message.contains("parent mesh retained")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id == "rhino:object:record#000000")
        .expect("future SubD proxy object record is retained");
    assert_eq!(retained.data.as_deref(), Some(mesh_record.as_slice()));
    assert_valid(&result);
}

#[test]
fn brep_region_userdata_future_payload_retains_parent_brep_record() {
    let archive = ArchiveVersion::V5;
    let future_region_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::brep::V5_BREP_REGION_TOPOLOGY_USERDATA.to_wire(),
        crate::wire::Uuid::from_canonical([
            0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe,
            0x52, 0x0d,
        ])
        .to_wire(),
        50,
        0,
        &future_region_payload,
    );
    let object_type = crate::test_support::test_dump::short_chunk(archive, 0x8200_0071, 0x10);
    let mut uuid_body = support::BREP_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&support::BREP_CLASS).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let mut brep_payload = support::brep_payload(false);
    brep_payload[0] = 0x32;
    let class_data = crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffc, &brep_payload);
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, userdata, class_end].concat(),
    );
    let object_end = crate::test_support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    let brep_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | crate::chunks::TCODE_CRC,
        &[object_type, class, object_end].concat(),
    );
    let following_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let bytes = support::archive(&[brep_record.clone(), following_point]);
    let result = decode(bytes);

    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("invalid optional Brep region topology discarded")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id == "rhino:object:record#000000")
        .expect("future Brep userdata object record is retained");
    assert_eq!(retained.data.as_deref(), Some(brep_record.as_slice()));
    assert_valid(&result);
}

#[test]
fn brep_nested_mesh_userdata_future_payload_retains_parent_record() {
    let archive = ArchiveVersion::V5;
    let future_ngon_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::mesh::V4V5_MESH_NGON_USERDATA.to_wire(),
        crate::wire::Uuid::from_canonical([
            0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe,
            0x52, 0x0d,
        ])
        .to_wire(),
        50,
        0,
        &future_ngon_payload,
    );
    let mut nested_uuid_body = support::MESH_CLASS.to_vec();
    nested_uuid_body.extend(crc32fast::hash(&support::MESH_CLASS).to_le_bytes());
    let nested_uuid = support::long_chunk(0x0002_fffb, &nested_uuid_body);
    let nested_class_data =
        support::crc_chunk(0x0002_fffc, &support::mesh_payload(3, 5, false, true));
    let nested_class_end = support::short_chunk(0x0002_7fff, 0);
    let nested_class = support::long_chunk(
        0x0002_7ffa,
        &[nested_uuid, nested_class_data, userdata, nested_class_end].concat(),
    );
    let mut mesh_side_body = vec![1_u8];
    mesh_side_body.extend(nested_class);
    let nested_class_range = 1..mesh_side_body.len();
    let mesh_side = crate::test_support::test_dump::crc_chunk_excluding(
        archive,
        0x4000_8000,
        &mesh_side_body,
        std::slice::from_ref(&nested_class_range),
    );
    let empty_mesh_side = support::crc_chunk(0x4000_8000, &[0]);
    let mut brep_payload = support::brep_payload(false);
    let payload_end = brep_payload.len();
    let inline_region_start = (0..payload_end)
        .rev()
        .find(|offset| {
            crate::chunks::chunk_at(&brep_payload, *offset, payload_end, archive, false).is_ok_and(
                |chunk| chunk.typecode == 0x4000_8000 && chunk.next_offset == payload_end,
            )
        })
        .expect("Brep fixture has an inline region wrapper");
    brep_payload.truncate(inline_region_start);
    brep_payload[0] = 0x32;
    let empty_positions = brep_payload
        .windows(empty_mesh_side.len())
        .enumerate()
        .filter_map(|(index, window)| (window == empty_mesh_side).then_some(index))
        .collect::<Vec<_>>();
    assert!(empty_positions.len() >= 2);
    let replace_at = empty_positions[empty_positions.len() - 2];
    brep_payload.splice(replace_at..replace_at + empty_mesh_side.len(), mesh_side);
    let object_type = crate::test_support::test_dump::short_chunk(archive, 0x8200_0071, 0x10);
    let mut uuid_body = support::BREP_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&support::BREP_CLASS).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data = crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffc, &brep_payload);
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, class_end].concat(),
    );
    let object_end = crate::test_support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    let brep_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | crate::chunks::TCODE_CRC,
        &[object_type, class, object_end].concat(),
    );
    let following_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let bytes = support::archive(&[brep_record.clone(), following_point]);
    let result = decode(bytes);

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("V4/V5 mesh n-gon userdata") && loss.message.contains("dropped")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id == "rhino:object:record#000000")
        .expect("nested mesh userdata object record is retained");
    assert_eq!(retained.data.as_deref(), Some(brep_record.as_slice()));
    assert_valid(&result);
}

#[test]
fn extrusion_display_mesh_cache_nested_userdata_future_payload_retains_parent_record() {
    let archive = ArchiveVersion::V5;
    let future_ngon_payload = crate::test_support::test_dump::crc_chunk(
        archive,
        0x4000_8000,
        &[
            2_i32.to_le_bytes().as_slice(),
            0_i32.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let mesh_userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::mesh::V4V5_MESH_NGON_USERDATA.to_wire(),
        crate::wire::Uuid::from_canonical([
            0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe,
            0x52, 0x0d,
        ])
        .to_wire(),
        50,
        0,
        &future_ngon_payload,
    );
    let mut mesh_uuid_body = support::MESH_CLASS.to_vec();
    mesh_uuid_body.extend(crc32fast::hash(&support::MESH_CLASS).to_le_bytes());
    let mesh_uuid =
        crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &mesh_uuid_body);
    let mesh_class_data = crate::test_support::test_dump::crc_chunk(
        archive,
        0x0002_fffc,
        &support::mesh_payload(3, 5, false, true),
    );
    let mesh_class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let render_mesh = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[mesh_uuid, mesh_class_data, mesh_userdata, mesh_class_end].concat(),
    );
    let null_uuid = crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffb, &[0; 16]);
    let null_mesh = crate::test_support::test_dump::long_chunk(archive, 0x0002_7ffa, &null_uuid);
    let cache_payload = [render_mesh, null_mesh.clone(), null_mesh].concat();
    let cache_userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::wire::Uuid::from_canonical([
            0xa8, 0x13, 0x0a, 0x3e, 0xe4, 0xf3, 0x4c, 0xb0, 0xbb, 0x8a, 0xf1, 0x0a, 0x47, 0x39,
            0x12, 0xd0,
        ])
        .to_wire(),
        crate::wire::Uuid::from_canonical([
            0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc,
            0x30, 0xd4,
        ])
        .to_wire(),
        50,
        0,
        &cache_payload,
    );
    let extrusion_payload =
        crate::extrusion::tests::archive_payload(2, [false, false], false, false);
    let object_type = crate::test_support::test_dump::short_chunk(archive, 0x8200_0071, 0x10);
    let mut uuid_body = support::EXTRUSION_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&support::EXTRUSION_CLASS).to_le_bytes());
    let class_uuid = crate::test_support::test_dump::long_chunk(archive, 0x0002_fffb, &uuid_body);
    let class_data =
        crate::test_support::test_dump::crc_chunk(archive, 0x0002_fffc, &extrusion_payload);
    let class_end = crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0);
    let class = crate::test_support::test_dump::long_chunk(
        archive,
        0x0002_7ffa,
        &[class_uuid, class_data, cache_userdata, class_end].concat(),
    );
    let object_end = crate::test_support::test_dump::short_chunk(archive, 0x8200_007f, 0);
    let extrusion_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_8070 | crate::chunks::TCODE_CRC,
        &[object_type, class, object_end].concat(),
    );
    let following_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let bytes = support::archive(&[extrusion_record.clone(), following_point]);
    let result = decode(bytes);

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains("V4/V5 mesh n-gon userdata") && loss.message.contains("dropped")
    }));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| record.id == "rhino:object:record#000000")
        .expect("extrusion cache object record is retained");
    assert_eq!(retained.data.as_deref(), Some(extrusion_record.as_slice()));
    assert_valid(&result);
}

#[test]
fn mapping_crc_cache_future_payload_retains_texture_mapping_owner() {
    let archive = ArchiveVersion::V5;
    let mapping_class = crate::presentation::TEXTURE_MAPPING.to_wire();
    let mapping_cache_class = crate::presentation::MAPPING_CRC_CACHE.to_wire();
    let opennurbs_application = crate::wire::Uuid::from_canonical([
        0x94, 0x21, 0xee, 0x97, 0xe8, 0x95, 0x47, 0xbc, 0x99, 0xeb, 0x5f, 0xd1, 0xba, 0x35, 0xb3,
        0x67,
    ])
    .to_wire();
    let mut cache_payload = 2_i32.to_le_bytes().to_vec();
    cache_payload.extend(0x1234_5678_i32.to_le_bytes());
    cache_payload.extend([0xde, 0xad]);
    let cache_userdata = crate::test_support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        mapping_cache_class,
        opennurbs_application,
        50,
        0,
        &cache_payload,
    );
    let primitive = crate::test_support::test_dump::class_wrapper_with_userdata(
        archive,
        crate::test_support::MESH_CLASS,
        &support::mesh_payload(3, 5, false, true),
        &cache_userdata,
    );
    let mut mapping_payload = [0x11_u8; 16].to_vec();
    mapping_payload.extend(6_u32.to_le_bytes());
    mapping_payload.extend(1_u32.to_le_bytes());
    for _ in 0..2 {
        for index in 0..16 {
            mapping_payload.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
    }
    mapping_payload.extend(crate::test_support::test_dump::utf16_bytes(
        "custom mesh mapping",
    ));
    mapping_payload.extend(primitive);
    mapping_payload.extend(3_u32.to_le_bytes());
    mapping_payload.push(0);
    mapping_payload.extend([0xaa, 0xbb]);
    let mapping_class_wrapper = crate::test_support::test_dump::class_wrapper(
        archive,
        mapping_class,
        &crate::test_support::test_dump::anonymous_chunk(archive, 1, &mapping_payload),
    );
    let mapping_record = crate::test_support::test_dump::nested_crc_chunk(
        archive,
        0x2000_807a,
        &mapping_class_wrapper,
    );
    let valid_point = support::object_record(
        1,
        support::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &[units]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0025,
                std::slice::from_ref(&mapping_record),
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    let texture_mappings =
        &result.ir().native.namespace("rhino").unwrap().arenas["texture_mappings"];
    assert_eq!(texture_mappings.len(), 1);
    assert_eq!(
        texture_mappings[0]
            .field("name")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("custom mesh mapping".to_owned())
    );
    assert_eq!(
        texture_mappings[0]
            .field("primitive_class_uuid")
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some(crate::wire::Uuid::from_wire(crate::test_support::MESH_CLASS).to_string())
    );
    assert!(texture_mappings[0].field("mapping_crc").is_none());
    assert_eq!(result.ir().model.points.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("MappingCRCCache")
            && loss.message.contains("could not be transferred")));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000025-2000807a-")
        })
        .expect("future mapping cache record is retained");
    assert_eq!(retained.data.as_deref(), Some(mapping_record.as_slice()));
    assert_valid(&result);
}

#[test]
fn future_settings_payload_is_retained_without_known_prefix() {
    let archive = ArchiveVersion::V5;
    let future_annotation =
        crate::test_support::test_dump::crc_chunk(archive, 0x2000_8034, &[0x20]);
    let valid_point = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &support::point_payload([4.0, 5.0, 6.0]),
    );
    let mut units_body = 100_i32.to_le_bytes().to_vec();
    units_body.extend(2_i32.to_le_bytes());
    units_body.extend(0.01_f64.to_le_bytes());
    units_body.extend(0.1_f64.to_le_bytes());
    units_body.extend(0.001_f64.to_le_bytes());
    let units = crate::test_support::test_dump::crc_chunk(archive, 0x2000_8031, &units_body);
    let bytes = crate::test_support::test_dump::minimal_document(
        "50",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0015,
                &[units, future_annotation.clone()],
            ),
            crate::test_support::test_dump::table(archive, 0x1000_0013, &[valid_point]),
        ],
    );
    let result = decode(bytes);

    assert_eq!(result.ir().model.points.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|record| {
            record
                .id
                .starts_with("rhino:opaque:record#10000015-20008034-")
        })
        .expect("future settings payload is retained");
    assert_eq!(retained.data.as_deref(), Some(future_annotation.as_slice()));
    assert_valid(&result);
}

/// Object types from `docs/formats/rhino_3dm.md` "object type values".
const HATCH_OBJECT_TYPE: i64 = 0x0001_0000;
const CURVE_OBJECT_TYPE: i64 = 0x0000_0004;

/// A synthesized archive whose two records both reach a native-retention path.
fn native_retention_archive() -> Vec<u8> {
    let hatch = support::object_record(
        HATCH_OBJECT_TYPE,
        crate::hatch::CLASS.to_wire(),
        &crate::hatch::tests::version_two_hatch_payload(),
    );
    let polyedge = support::object_record(
        CURVE_OBJECT_TYPE,
        crate::polyedge::CURVE_CLASS.to_wire(),
        &crate::polyedge::tests::polyedge_payload(),
    );
    support::archive(&[hatch, polyedge])
}

#[test]
fn native_retentions_are_charged_and_excluded_from_the_decoded_census() {
    let result = decode(native_retention_archive());
    let losses = &result.report().losses;

    assert!(losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ObjectRecordCensus.kind()
            && loss.message.contains("decoded 0/2 Rhino object records")
    }));

    for code in [
        crate::loss::RhinoLossCode::HatchFillNotTransferred,
        crate::loss::RhinoLossCode::PolyedgeReferencesNotResolved,
    ] {
        let charged = losses
            .iter()
            .filter(|loss| loss.code == code.kind())
            .collect::<Vec<_>>();
        assert_eq!(charged.len(), 1, "{} not charged once", code.code());
        assert_eq!(charged[0].code, code.kind());
        assert_eq!(charged[0].severity, Severity::Warning);
        assert!(charged[0]
            .message
            .contains("framed and read 1 object record"));
        assert!(charged[0].provenance.is_some());
    }

    assert!(!losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ObjectFamilyNotTransferred.kind()
    }));

    // The hatch loop curve is a real neutral carrier even though the fill is not.
    assert!(!result.ir().model.curves.is_empty());
    assert_eq!(result.ir().model.features.len(), 2);
    assert!(result.report().geometry_transferred);
    assert_valid(&result);
}

#[test]
fn user_table_records_are_retained_as_complete_opaque_source_records() {
    let record = support::long_chunk(0x7000_0042, b"plug-in application bytes");
    let expected = record.clone();
    let result = decode(support::archive_with_user_records(&[], &[record]));
    let retained = result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|value| value.id.starts_with("rhino:opaque:record#"))
        .expect("user table record must be retained");
    assert!(retained.id.contains("-70000042-"));
    assert_eq!(retained.byte_len, expected.len() as u64);
    assert_eq!(retained.data.as_deref(), Some(expected.as_slice()));
}

/// Object type for annotation records, per `docs/formats/rhino_3dm.md`.
const ANNOTATION_OBJECT_TYPE: i64 = 0x0000_0200;
/// `unit_value` 2 in `test_support::archive` is millimeters.
const MILLIMETERS_PER_UNIT: f64 = 1.0;

/// A synthesized archive holding one linear dimension on a rotated plane.
///
/// `dimstyle_wire` selects whether the style reference is nil (an explicit null
/// reference) or a non-nil UUID with no decoded style record (charged, and no
/// dangling `target` emitted).
fn dimension_archive(dimstyle_wire: [u8; 16], text_point: Option<[f64; 2]>) -> Vec<u8> {
    // Definition point x drives the measurement; see the linear family layout.
    let family = [3.0_f64, 4.0, 8.0, 9.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect::<Vec<_>>();
    let plane = crate::dimensions::tests::plane_bytes(
        DIMENSION_PLANE_ORIGIN,
        DIMENSION_PLANE_X,
        DIMENSION_PLANE_Y,
        [0.0, 0.0, 1.0, -DIMENSION_PLANE_ORIGIN[2]],
    );
    support::archive(&[support::object_record(
        ANNOTATION_OBJECT_TYPE,
        crate::dimensions::LINEAR.to_wire(),
        &crate::dimensions::tests::dimension_payload(1, &family, dimstyle_wire, &plane, text_point),
    )])
}

const DIMENSION_PLANE_ORIGIN: [f64; 3] = [10.0, 20.0, 30.0];
const DIMENSION_PLANE_X: [f64; 3] = [0.0, 1.0, 0.0];
const DIMENSION_PLANE_Y: [f64; 3] = [-1.0, 0.0, 0.0];

#[test]
fn dimension_becomes_a_measured_semantic_annotation_with_resolvable_identities() {
    let text_point = [2.0, 3.0];
    let result = decode(dimension_archive([0; 16], Some(text_point)));
    assert_eq!(result.ir().model.semantic_annotations.len(), 1);
    assert!(result.ir().model.parameters.is_empty());
    assert!(result.ir().model.features.is_empty());

    let annotation = &result.ir().model.semantic_annotations[0];
    assert_eq!(annotation.kind, SemanticAnnotationKind::Dimension);
    assert_eq!(annotation.runtime_type, "linear_dimension");

    // Constraint 1: `object` and `native_ref` resolve against the document.
    let record = &result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[0];
    assert_eq!(annotation.object, record.id.to_string());
    assert_eq!(annotation.native_ref, record.id.to_string());
    assert!(record.links.contains(&annotation.id.0));

    // Constraint 2: `order` is a dense u32 arena index, not the byte offset.
    assert_eq!(annotation.order, 0);

    // Constraint 3: a nil style UUID is an explicit null reference, never a
    // dangling target.
    for role in ["dimstyle_id", "detail_measured"] {
        let targets = &annotation.references[role];
        assert_eq!(targets.len(), 1);
        assert!(targets[0].is_null);
        assert!(targets[0].target.is_none());
    }
    assert!(annotation.assets.is_empty());

    // Constraint 4: the plane-space text point is composed, not lifted with z=0.
    let expected = [
        DIMENSION_PLANE_ORIGIN[0] * MILLIMETERS_PER_UNIT
            + text_point[0] * DIMENSION_PLANE_X[0]
            + text_point[1] * DIMENSION_PLANE_Y[0],
        DIMENSION_PLANE_ORIGIN[1] * MILLIMETERS_PER_UNIT
            + text_point[0] * DIMENSION_PLANE_X[1]
            + text_point[1] * DIMENSION_PLANE_Y[1],
        DIMENSION_PLANE_ORIGIN[2] * MILLIMETERS_PER_UNIT
            + text_point[0] * DIMENSION_PLANE_X[2]
            + text_point[1] * DIMENSION_PLANE_Y[2],
    ];
    let position = annotation.position.expect("authored text point");
    for axis in 0..3 {
        assert!(
            (position[axis] - expected[axis]).abs() < 1.0e-12,
            "{position:?} != {expected:?}"
        );
    }

    // The linear measurement is |definition_point.x| * distance_scale, with the
    // family's 3.0 and the record's 2.0 scale.
    let value = annotation.value.expect("persisted measurement");
    assert!(
        (value - 3.0 * 2.0 * MILLIMETERS_PER_UNIT).abs() < 1.0e-12,
        "{value}"
    );

    assert_valid(&result);
}

#[test]
fn unresolvable_dimension_style_is_charged_without_a_dangling_reference() {
    let dimstyle = [0x11; 16];
    let result = decode(dimension_archive(dimstyle, None));
    let annotation = &result.ir().model.semantic_annotations[0];
    assert!(!annotation.references.contains_key("dimstyle_id"));
    assert_eq!(
        annotation.parameters["dimstyle_id"],
        crate::wire::Uuid::from_wire(dimstyle).to_string()
    );
    assert!(annotation.position.is_none());

    let charged = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::RhinoLossCode::DimensionStyleUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(charged.len(), 1);
    assert_eq!(
        charged[0].code,
        crate::loss::RhinoLossCode::DimensionStyleUnresolved.kind()
    );
    assert_valid(&result);
}

#[test]
fn several_dimensions_take_dense_unique_orders_independent_of_byte_offsets() {
    let family = [3.0_f64, 4.0, 8.0, 9.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect::<Vec<_>>();
    let record = |annotation_type: i32| {
        support::object_record(
            ANNOTATION_OBJECT_TYPE,
            crate::dimensions::LINEAR.to_wire(),
            &crate::dimensions::tests::dimension_payload(
                annotation_type,
                &family,
                [0; 16],
                &crate::dimensions::tests::plane_bytes(
                    DIMENSION_PLANE_ORIGIN,
                    DIMENSION_PLANE_X,
                    DIMENSION_PLANE_Y,
                    [0.0, 0.0, 1.0, -DIMENSION_PLANE_ORIGIN[2]],
                ),
                None,
            ),
        )
    };
    // Annotation types 1 and 5 are both linear, so the two records differ in
    // content and therefore in length: byte offsets are not a dense sequence.
    let result = decode(support::archive(&[record(1), record(5), record(1)]));
    let orders = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .map(|annotation| annotation.order)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(orders, std::collections::BTreeSet::from([0, 1, 2]));
    assert_valid(&result);
}

#[test]
fn typed_class_constants_preserve_canonical_uuid_display() {
    assert_eq!(
        crate::mesh::ON_MESH.to_string(),
        "4ed7d4e4-e947-11d3-bfe5-0010830122f0"
    );
    assert_eq!(
        crate::brep::ON_BREP.to_string(),
        "60b5dbc5-e660-11d3-bfe4-0010830122f0"
    );
    assert_eq!(
        crate::extrusion::ON_EXTRUSION.to_string(),
        "36f53175-72b8-4d47-bf1f-b4e6fc24f4b9"
    );
    assert_eq!(
        crate::subd::ON_SUBD.to_string(),
        "f09ba4d9-455b-42c3-ba3b-e6ccacef853b"
    );
}

const BASELINE: [(u64, usize, usize); 7] = [
    (2, 1989, 2342),
    (3, 2413, 2477),
    (4, 47, 173),
    (50, 92, 198),
    (60, 28, 37),
    (70, 31, 46),
    (80, 24, 39),
];

const STRUCTURED_SOURCE_OBJECTS: usize = 6;
const STRUCTURED_TRANSFER: [(u64, usize, usize); 4] = [
    (50, 6, STRUCTURED_SOURCE_OBJECTS),
    (60, 6, STRUCTURED_SOURCE_OBJECTS),
    (70, 6, STRUCTURED_SOURCE_OBJECTS),
    (80, 6, STRUCTURED_SOURCE_OBJECTS),
];

fn files(root: &Path, output: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(root)
        .expect("read openNURBS example directory")
        .map(|entry| entry.expect("read openNURBS example entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "3dm") {
            output.push(path);
        }
    }
}

fn note_count(notes: &[String]) -> Option<(usize, usize)> {
    notes.iter().find_map(|note| {
        let rest = note.strip_prefix("decoded ")?;
        let fraction = rest.split_whitespace().next()?;
        let (decoded, total) = fraction.split_once('/')?;
        Some((decoded.parse().ok()?, total.parse().ok()?))
    })
}

fn archive_version(notes: &[String]) -> Option<u64> {
    notes
        .iter()
        .find_map(|note| note.strip_prefix("archive version ")?.parse().ok())
}

fn decode_counts(path: &Path) -> Option<(u64, usize, usize)> {
    let bytes = fs::read(path).expect("read 3DM witness");
    let inspect = RhinoCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .expect("inspect witness");
    let version = archive_version(&inspect.notes).expect("archive version note");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .ok()?;
    if version == 1 {
        // Version 1 is a documented L0 boundary. Keep the reader traversal in
        // the external witness, but do not claim object transfer for it.
        return None;
    }
    let (supported, total) = note_count(&decoded.report().notes).unwrap_or((0, 0));
    if supported < total && std::env::var_os("RHINO_WITNESS_DIAGNOSTICS").is_some() {
        eprintln!("{}: {supported}/{total}", path.display());
        for loss in &decoded.report().losses {
            eprintln!("  {}: {}", loss.code, loss.message);
        }
    }
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(
        validation.findings.iter().all(|finding| !matches!(
            finding.severity,
            cadmpeg_ir::report::Severity::Error | cadmpeg_ir::report::Severity::Blocking
        )),
        "validation failed for {}",
        path.display()
    );
    Some((version, supported, total))
}

fn oracle_object_count(output: &[u8]) -> usize {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let suffix = line.trim().strip_prefix("ModelGeometry ")?;
            suffix.strip_suffix(':')?.parse::<usize>().ok()
        })
        .max()
        .map_or(0, |index| index + 1)
}

#[test]
#[ignore = "requires OPENNURBS_ROOT and an openNURBS example_read executable"]
fn opennurbs_object_walk_and_transfer_floor() {
    let root = PathBuf::from(std::env::var_os("OPENNURBS_ROOT").expect("OPENNURBS_ROOT"));
    let reader = root.join("example_read/example_read");
    assert!(reader.is_file(), "build openNURBS example_read first");
    let mut inputs = Vec::new();
    files(&root.join("example_files"), &mut inputs);
    assert_eq!(inputs.len(), 153, "unexpected openNURBS example corpus");

    let mut counts = BTreeMap::<u64, (usize, usize)>::new();
    for path in inputs {
        let witness = Command::new(&reader)
            .arg(&path)
            .output()
            .expect("run openNURBS example_read");
        assert!(
            witness.status.success(),
            "example_read refused {}",
            path.display()
        );
        let oracle_total = oracle_object_count(&witness.stdout);

        let Some((version, supported, total)) = decode_counts(&path) else {
            continue;
        };
        if total > 0 {
            assert_eq!(
                total,
                oracle_total,
                "object walk differs for {}",
                path.display()
            );
        }
        let entry = counts.entry(version).or_default();
        entry.0 += supported;
        entry.1 += total;
    }

    if std::env::var_os("RHINO_WITNESS_DIAGNOSTICS").is_some() {
        for (version, actual) in &counts {
            eprintln!("archive {version}: {}/{}", actual.0, actual.1);
        }
    }
    for (version, minimum_supported, expected_total) in BASELINE {
        let actual = counts.get(&version).copied().unwrap_or_default();
        assert_eq!(
            actual.1, expected_total,
            "archive {version} object-walk drift"
        );
        assert!(
            actual.0 >= minimum_supported,
            "archive {version} transfer regressed: {} < {minimum_supported}",
            actual.0
        );
    }

    let generated =
        PathBuf::from(std::env::var_os("OPENNURBS_SYNTH_DIR").expect("OPENNURBS_SYNTH_DIR"));
    for version in [50, 60, 70, 80] {
        let point_path = generated.join(format!("witness-v{version}-point.3dm"));
        let witness = Command::new(&reader)
            .arg(&point_path)
            .output()
            .expect("run example_read on synthesized witness");
        assert!(
            witness.status.success(),
            "example_read refused {}",
            point_path.display()
        );
        assert_eq!(decode_counts(&point_path), Some((version, 1, 1)));
    }

    for (version, expected_supported, expected_total) in STRUCTURED_TRANSFER {
        let path = generated.join(format!("witness-v{version}-structured.3dm"));
        let witness = Command::new(&reader)
            .arg(&path)
            .output()
            .expect("run example_read on structured synthesized witness");
        assert!(
            witness.status.success(),
            "example_read refused {}",
            path.display()
        );
        assert_eq!(oracle_object_count(&witness.stdout), expected_total);
        assert_eq!(
            decode_counts(&path),
            Some((version, expected_supported, expected_total))
        );
    }

    for version in [50, 60, 70, 80] {
        let archive_version = match version {
            50 => RhinoArchiveVersion::V5,
            60 => RhinoArchiveVersion::V6,
            70 => RhinoArchiveVersion::V7,
            80 => RhinoArchiveVersion::V8,
            _ => unreachable!("supported writer version table"),
        };
        let mut point_ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        point_ir.model.points.push(cadmpeg_ir::topology::Point {
            id: cadmpeg_ir::ids::PointId("integration:writer-point#0".into()),
            position: cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75),
            source_object: None,
        });
        let mut bytes = Vec::new();
        RhinoEncoder::new(archive_version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &point_ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("write codec point witness");
        let path = generated.join(format!("codec-writer-v{version}-point.3dm"));
        fs::write(&path, bytes).expect("write codec point witness file");

        let witness = Command::new(&reader)
            .arg(&path)
            .output()
            .expect("run example_read on codec writer witness");
        assert!(
            witness.status.success(),
            "example_read refused {}",
            path.display()
        );
        assert_eq!(oracle_object_count(&witness.stdout), 1);
        assert_eq!(decode_counts(&path), Some((version, 1, 1)));
    }
}
