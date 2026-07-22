// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::report::Severity;
use sha2::{Digest, Sha256};

use super::archive_test_support::{
    arc_payload, archive, archive_unit, archive_version, archive_writer, brep_payload,
    line_payload, mesh_payload, object_record, point_cloud_payload, point_payload,
    polycurve_payload, polyline_payload, singular_seam_brep_payload, ARC_CLASS, BREP_CLASS,
    EXTRUSION_CLASS, LINE_CLASS, MESH_CLASS, POINT_CLASS, POINT_CLOUD_CLASS, POLYCURVE_CLASS,
    POLYLINE_CLASS, SUBD_CLASS,
};
use super::RhinoCodec;

fn decode(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("complete synthetic archive")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

#[test]
fn open_nurbs_object_record_short_typecodes_decode() {
    let record = object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    assert_eq!(&record[12..16], &0x8200_0071_u32.to_le_bytes());
    assert_eq!(
        &record[record.len() - 16..record.len() - 12],
        &0x8200_007f_u32.to_le_bytes()
    );

    let result = decode(&archive(&[record]));

    assert_eq!(result.ir.model.points.len(), 1, "{:?}", result.report);
    assert!(result.report.geometry_transferred);
}

#[test]
fn complete_point_and_bounded_line_archive_decodes_semantics_and_links() {
    let point = object_record(1, POINT_CLASS, &point_payload([1.25, -2.5, 3.75]));
    let line = object_record(
        4,
        LINE_CLASS,
        &line_payload([10.0, 20.0, 30.0], [2.0, 0.0, 0.0], [3.0, 7.0]),
    );
    let result = decode(&archive(&[point, line]));

    assert_eq!(result.ir.model.points.len(), 1, "{:?}", result.report);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    assert_eq!(result.ir.model.curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &result.ir.model.curves[0].geometry else {
        panic!("bounded line must decode to an exact NURBS carrier");
    };
    assert_eq!(curve.degree, 1);
    assert_eq!(curve.knots, vec![3.0, 3.0, 7.0, 7.0]);
    assert_eq!(
        curve.control_points,
        vec![
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
        ]
    );
    assert_eq!(result.ir.native_unknowns("rhino").unwrap().len(), 2);
    assert!(result.ir.native_unknowns("rhino").unwrap()[0]
        .links
        .contains(&result.ir.model.bodies[0].id.to_string()));
    assert!(result.ir.native_unknowns("rhino").unwrap()[1]
        .links
        .contains(&result.ir.model.curves[0].id.to_string()));
    assert!(result.report.geometry_transferred);
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn future_and_semantically_invalid_objects_are_atomic_and_later_point_recovers() {
    for malformed in [
        object_record(1, POINT_CLASS, &[0x20]),
        object_record(1, POINT_CLASS, &point_payload([f64::NAN, 0.0, 0.0])),
    ] {
        let valid = object_record(1, POINT_CLASS, &point_payload([4.0, 5.0, 6.0]));
        let result = decode(&archive(&[malformed, valid]));

        assert_eq!(result.ir.model.points.len(), 1, "{:?}", result.report);
        assert_eq!(
            result.ir.model.points[0].position,
            cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
        );
        assert!(result.ir.native_unknowns("rhino").unwrap()[0]
            .links
            .is_empty());
        assert!(!result.ir.native_unknowns("rhino").unwrap()[1]
            .links
            .is_empty());
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.severity != Severity::Info));
        assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
    }
}

#[test]
fn retention_caps_store_only_complete_records_with_exact_hashes() {
    let large = object_record(1, [0; 16], &[0x55; 64]);
    let point = object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let bytes = archive(&[large.clone(), point.clone()]);
    let scan = super::container::scan(bytes).expect("complete archive scan");
    let mut context = super::decode::DecodeContext::new(&scan);
    context.set_retention_limits(point.len(), point.len());
    let result = context.commit();

    let retained = &result.source_fidelity.retained_records;
    assert_eq!(retained[0].byte_len, large.len() as u64);
    assert_eq!(retained[0].sha256, sha256_hex(&large));
    assert_eq!(retained[0].data, None);
    assert_eq!(retained[1].byte_len, point.len() as u64);
    assert_eq!(retained[1].sha256, sha256_hex(&point));
    assert_eq!(retained[1].data.as_deref(), Some(point.as_slice()));

    let two_points = archive(&[point.clone(), point.clone()]);
    let scan = super::container::scan(two_points).expect("complete archive scan");
    let mut context = super::decode::DecodeContext::new(&scan);
    context.set_retention_limits(point.len(), point.len());
    let result = context.commit();
    assert_eq!(
        result.source_fidelity.retained_records[0].data.as_deref(),
        Some(point.as_slice())
    );
    assert_eq!(result.source_fidelity.retained_records[1].data, None);
}

#[test]
fn repeat_decode_is_byte_deterministic_for_ir_and_report() {
    let bytes = archive(&[
        object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
        object_record(
            4,
            LINE_CLASS,
            &line_payload([0.0, 1.0, 2.0], [0.0, 3.0, 0.0], [-2.0, 4.0]),
        ),
    ]);
    let first = decode(&bytes);
    let second = decode(&bytes);

    assert_eq!(
        first.ir.to_canonical_json().unwrap().as_bytes(),
        second.ir.to_canonical_json().unwrap().as_bytes()
    );
    assert_eq!(
        serde_json::to_vec(&first.report).unwrap(),
        serde_json::to_vec(&second.report).unwrap()
    );
}

#[test]
fn subd_complete_object_commits_across_supported_archive_bands() {
    for (version, archive_band) in [
        ("50", super::chunks::ArchiveVersion::V5),
        ("60", super::chunks::ArchiveVersion::V6),
        ("70", super::chunks::ArchiveVersion::V7),
        ("80", super::chunks::ArchiveVersion::V8),
    ] {
        let object = object_record(
            0x0004_0000,
            SUBD_CLASS,
            &super::subd::tests::quad_payload(archive_band),
        );
        let result = decode(&archive_version(version, &[object]));
        assert_eq!(result.ir.model.subds.len(), 1, "archive {version}");
        assert_eq!(result.ir.model.subds[0].faces[0].edges.len(), 4);
        assert!(!result.ir.native_unknowns("rhino").unwrap()[0]
            .links
            .is_empty());
        assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
    }
}

#[test]
fn complete_simple_geometry_archive_preserves_coordinates_knots_and_compound_order() {
    let inner = polycurve_payload(
        &[10.0, 20.0],
        &[(
            LINE_CLASS,
            line_payload([2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [10.0, 20.0]),
        )],
    );
    let outer = polycurve_payload(
        &[0.0, 2.0, 5.0],
        &[
            (
                LINE_CLASS,
                line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0]),
            ),
            (POLYCURVE_CLASS, inner),
        ],
    );
    let result = decode(&archive(&[
        object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
        object_record(
            2,
            POINT_CLOUD_CLASS,
            &point_cloud_payload(&[[4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]),
        ),
        object_record(
            4,
            LINE_CLASS,
            &line_payload([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [-1.0, 3.0]),
        ),
        object_record(
            4,
            ARC_CLASS,
            &arc_payload([0.0, std::f64::consts::FRAC_PI_2], [4.0, 9.0]),
        ),
        object_record(
            4,
            POLYLINE_CLASS,
            &polyline_payload(
                &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [4.0, 2.0, 0.0]],
                &[2.0, 3.5, 9.0],
            ),
        ),
        object_record(4, POLYCURVE_CLASS, &outer),
    ]));

    assert_eq!(result.ir.model.points.len(), 3);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
    assert_eq!(
        result.ir.model.points[2].position,
        cadmpeg_ir::math::Point3::new(7.0, 8.0, 9.0)
    );
    assert_eq!(result.ir.model.curves.len(), 7);
    let line = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| {
            matches!(
                &curve.geometry,
                CurveGeometry::Nurbs(nurbs)
                    if nurbs.knots == [-1.0, -1.0, 3.0, 3.0]
            )
        })
        .expect("bounded line");
    let CurveGeometry::Nurbs(line) = &line.geometry else {
        panic!("line carrier");
    };
    assert_eq!(line.knots, vec![-1.0, -1.0, 3.0, 3.0]);
    let polyline = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| {
            matches!(
                &curve.geometry,
                CurveGeometry::Nurbs(nurbs)
                    if nurbs.knots == [2.0, 2.0, 3.5, 9.0, 9.0]
            )
        })
        .expect("polyline");
    let CurveGeometry::Nurbs(polyline) = &polyline.geometry else {
        panic!("polyline carrier");
    };
    assert_eq!(polyline.knots, vec![2.0, 2.0, 3.5, 9.0, 9.0]);
    assert_eq!(result.ir.model.procedural_curves.len(), 2);
    let root = result
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| !curve.id.as_str().contains("component"))
        .expect("root compound");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
        parameters,
        components,
        ..
    } = &root.definition
    else {
        panic!("compound definition");
    };
    assert_eq!(parameters, &vec![0.0, 2.0, 5.0]);
    assert_eq!(components.len(), 2);
    assert!(components[0].as_str().contains("component-0"));
    assert!(components[1].as_str().contains("component-1"));
    assert_eq!(result.ir.native_unknowns("rhino").unwrap().len(), 6);
    assert!(result
        .ir
        .native_unknowns("rhino")
        .unwrap()
        .iter()
        .all(|record| !record.links.is_empty()));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn serialized_mesh_major_and_minor_matrix_reaches_object_dispatch() {
    for major in [1_u8, 3] {
        for minor in 0_u8..=8 {
            let archive_band = if major == 3 && minor > 5 { "60" } else { "50" };
            let mapping = major == 3 && minor >= 4;
            let object = object_record(
                0x20,
                MESH_CLASS,
                &mesh_payload(major, minor, false, mapping),
            );
            let bytes = if mapping {
                archive_writer(archive_band, 200_606_010, &[object])
            } else {
                archive_version(archive_band, &[object])
            };
            let result = decode(&bytes);
            assert_eq!(
                result.ir.model.tessellations.len(),
                1,
                "major {major} minor {minor}: {:?}",
                result.report
            );
            let mesh = &result.ir.model.tessellations[0];
            assert_eq!(mesh.vertices.len(), 4);
            assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 2, 3], [0, 3, 1]]);
            assert_eq!(mesh.normals.len(), 4);
            assert!(mesh
                .channels
                .iter()
                .any(|channel| channel.kind == 0x5248_0001));
            assert!(mesh
                .channels
                .iter()
                .any(|channel| channel.kind == 0x5248_0002));
            if major == 3 && minor >= 3 {
                assert!(mesh
                    .channels
                    .iter()
                    .any(|channel| channel.kind == 0x5248_0003));
            }
            assert_eq!(
                mesh.vertices[2],
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0)
            );
            assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
        }
    }
}

#[test]
fn required_mesh_channel_failure_is_atomic_and_optional_crc_is_recoverable() {
    let bad_mesh = object_record(0x20, MESH_CLASS, &mesh_payload(3, 5, true, false));
    let point = object_record(1, POINT_CLASS, &point_payload([8.0, 9.0, 10.0]));
    let result = decode(&archive(&[bad_mesh, point]));
    assert!(result.ir.model.tessellations.is_empty());
    assert_eq!(result.ir.model.points.len(), 1);
    assert!(result.ir.native_unknowns("rhino").unwrap()[0]
        .links
        .is_empty());
    assert!(!result.ir.native_unknowns("rhino").unwrap()[1]
        .links
        .is_empty());
    let failure = result
        .report
        .losses
        .iter()
        .find(|loss| loss.severity == Severity::Error)
        .expect("mesh failure loss");
    assert_eq!(
        failure.message,
        "1 framed object record(s) for class 4ed7d4e4-e947-11d3-bfe5-0010830122f0 could not be decoded"
    );
    let provenance = failure.provenance.as_ref().expect("failure provenance");
    assert_eq!(provenance.format, "rhino");
    assert!(provenance
        .tag
        .as_deref()
        .is_some_and(|tag| tag.starts_with("OBJECT_RECORD/class=4ed7d4e4")));

    let mut optional_crc = mesh_payload(3, 5, false, false);
    let vertex_buffer = 1 + 8 + 64 + 16 + 64 + 4 + 5 + 4 + 8;
    let normal_buffer = vertex_buffer + 4 + 4 + 1 + 48;
    optional_crc[normal_buffer + 4..normal_buffer + 8].copy_from_slice(&0_u32.to_le_bytes());
    let result = decode(&archive(&[object_record(0x20, MESH_CLASS, &optional_crc)]));
    assert_eq!(
        result.ir.model.tessellations.len(),
        1,
        "{:?}",
        result.report
    );
    assert!(result.ir.model.tessellations[0].normals.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Warning && loss.message.contains("normals")));
}

#[test]
fn serialized_extrusion_versions_caps_holes_and_cache_dispatch_atomically() {
    for minor in 0..=3 {
        let payload = super::extrusion::tests::archive_payload(
            minor,
            [minor != 2, minor == 2],
            false,
            minor == 3,
        );
        let result = decode(&archive(&[object_record(0x10, EXTRUSION_CLASS, &payload)]));
        assert_eq!(
            result.ir.model.procedural_surfaces.len(),
            1,
            "minor {minor}: {:?}",
            result.report
        );
        let expected_caps = if minor < 2 { 2 } else { 1 };
        assert_eq!(result.ir.model.faces.len(), expected_caps);
        assert_eq!(result.ir.model.tessellations.len(), usize::from(minor == 3));
        assert!(!result.ir.native_unknowns("rhino").unwrap()[0]
            .links
            .is_empty());
        assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
    }

    let payload = super::extrusion::tests::archive_payload(2, [true, true], true, false);
    let result = decode(&archive(&[object_record(0x10, EXTRUSION_CLASS, &payload)]));
    assert_eq!(result.ir.model.procedural_surfaces.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 2);
    assert_eq!(result.ir.model.loops.len(), 4);
    assert_eq!(result.ir.model.pcurves.len(), 4);
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn invalid_extrusion_profile_is_one_unknown_surface_and_later_point_recovers() {
    let mut malformed = super::extrusion::tests::archive_payload(2, [true, false], false, false);
    let profile_count = malformed.len() - 4 - 2 - 4;
    malformed[profile_count..profile_count + 4].copy_from_slice(&2_i32.to_le_bytes());
    let body = &malformed[12..malformed.len() - 4];
    let crc = crc32fast::hash(body);
    let end = malformed.len();
    malformed[end - 4..].copy_from_slice(&crc.to_le_bytes());
    let point = object_record(1, POINT_CLASS, &point_payload([3.0, 4.0, 5.0]));
    let result = decode(&archive(&[
        object_record(0x10, EXTRUSION_CLASS, &malformed),
        point,
    ]));
    assert!(result.ir.model.procedural_surfaces.is_empty());
    assert!(result.ir.model.faces.is_empty());
    assert!(result.ir.model.pcurves.is_empty());
    assert_eq!(
        result
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Unknown { .. }
            ))
            .count(),
        1
    );
    assert_eq!(result.ir.model.points.len(), 1);
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn serialized_brep_l3_commits_connected_topology_pcurves_and_scaled_tolerances() {
    let payload = brep_payload(false);
    assert_eq!(payload[13], 0x10);
    super::brep::parse(
        &payload,
        0..payload.len(),
        super::chunks::ArchiveVersion::V5,
        None,
    )
    .expect("direct Brep fixture parse");
    let result = decode(&archive_unit(
        3,
        &[object_record(0x10, BREP_CLASS, &payload)],
    ));
    let model = &result.ir.model;
    assert_eq!(model.bodies.len(), 1, "{:?}", result.report);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.loops.len(), 1);
    assert_eq!(model.coedges.len(), 3);
    assert_eq!(model.edges.len(), 3);
    assert_eq!(model.vertices.len(), 3);
    assert_eq!(model.pcurves.len(), 3);
    assert_eq!(model.curves.len(), 3);
    assert_eq!(model.surfaces.len(), 1);
    let body = &model.bodies[0];
    let region = &model.regions[0];
    let shell = &model.shells[0];
    let face = &model.faces[0];
    let loop_record = &model.loops[0];
    assert_eq!(body.regions, vec![region.id.clone()]);
    assert_eq!(region.body, body.id);
    assert_eq!(region.shells, vec![shell.id.clone()]);
    assert_eq!(shell.region, region.id);
    assert_eq!(shell.faces, vec![face.id.clone()]);
    assert_eq!(face.shell, shell.id);
    assert_eq!(face.loops, vec![loop_record.id.clone()]);
    assert_eq!(loop_record.face, face.id);
    assert!(model.coedges.iter().all(|coedge| !coedge.pcurves.is_empty()
        && model.edges.iter().any(|edge| edge.id == coedge.edge)));
    assert!(model.edges.iter().all(|edge| edge.curve.is_some()
        && edge
            .tolerance
            .is_some_and(|tolerance| (tolerance - 0.3).abs() < 1.0e-12)));
    assert!(model.vertices.iter().all(|vertex| vertex
        .tolerance
        .is_some_and(|tolerance| (tolerance - 0.2).abs() < 1.0e-12)));
    assert!(model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.fit_tolerance == Some(0.04)));
    assert_eq!(result.ir.native_unknowns("rhino").unwrap().len(), 1);
    assert!(result.ir.native_unknowns("rhino").unwrap()[0]
        .links
        .contains(&body.id.to_string()));
    assert!(result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message == "decoded 1/1 Rhino object records"));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn serialized_singular_seam_ring_uses_directed_trim_vertices() {
    let valid = decode(&archive(&[object_record(
        0x10,
        BREP_CLASS,
        &singular_seam_brep_payload(false),
    )]));
    let model = &valid.ir.model;
    assert_eq!(model.bodies.len(), 1, "{:?}", valid.report);
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.loops.len(), 1);
    assert_eq!(model.coedges.len(), 4);
    assert_eq!(model.edges.len(), 3);
    assert_eq!(model.vertices.len(), 2);
    assert_eq!(model.pcurves.len(), 4);
    assert_eq!(
        model.coedges[3].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert!(cadmpeg_ir::validate(&valid.ir, valid.report.losses.clone()).is_ok());

    let malformed = decode(&archive(&[object_record(
        0x10,
        BREP_CLASS,
        &singular_seam_brep_payload(true),
    )]));
    assert!(malformed.ir.model.faces.is_empty());
    assert!(malformed.ir.model.loops.is_empty());
    assert!(malformed.ir.model.coedges.is_empty());
    assert!(malformed.report.losses.iter().any(|loss| {
        loss.severity == Severity::Warning
            && loss.message.contains("Brep topology fallback")
            && loss.message.contains("loop ring is discontinuous")
    }));
    assert!(cadmpeg_ir::validate(&malformed.ir, malformed.report.losses.clone()).is_ok());
}

#[test]
fn semantic_invalid_brep_keeps_only_free_c3_surface_and_later_point() {
    let point = object_record(1, POINT_CLASS, &point_payload([9.0, 8.0, 7.0]));
    let result = decode(&archive(&[
        object_record(0x10, BREP_CLASS, &brep_payload(true)),
        point,
    ]));
    let model = &result.ir.model;
    assert!(model.faces.is_empty());
    assert!(model.loops.is_empty());
    assert!(model.coedges.is_empty());
    assert!(model.edges.is_empty());
    assert!(model.pcurves.is_empty());
    assert_eq!(model.curves.len(), 3);
    assert_eq!(model.surfaces.len(), 1);
    assert_eq!(model.points.len(), 1);
    assert!(!result.ir.native_unknowns("rhino").unwrap()[0]
        .links
        .is_empty());
    assert!(!result.ir.native_unknowns("rhino").unwrap()[1]
        .links
        .is_empty());
    assert!(
        result.report.losses.iter().any(|loss| {
            loss.severity == Severity::Warning && loss.message.contains("Brep topology fallback")
        }),
        "{:?}",
        result.report
    );
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn archive_failure_recovery_matrix_preserves_exact_unknown_records() {
    let mut malformed_zlib = mesh_payload(3, 0, false, false);
    let common_bytes = 1 + 8 + 64 + 16 + 64 + 4 + 5 + 4 + 8;
    malformed_zlib[common_bytes + 8] = 1;

    let mut missing_child = polycurve_payload(
        &[0.0, 1.0, 2.0],
        &[(
            LINE_CLASS,
            line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        )],
    );
    missing_child[1..5].copy_from_slice(&2_i32.to_le_bytes());

    let failures = [
        object_record(1, POINT_CLASS, &[0x20]),
        object_record(0x20, MESH_CLASS, &malformed_zlib),
        object_record(4, POLYCURVE_CLASS, &missing_child),
        object_record(1, POINT_CLASS, &point_payload([f64::NAN, 0.0, 0.0])),
    ];
    for failure in failures {
        let point = object_record(1, POINT_CLASS, &point_payload([6.0, 7.0, 8.0]));
        let result = decode(&archive(&[failure.clone(), point]));
        assert_eq!(result.ir.model.points.len(), 1, "{:?}", result.report);
        assert_eq!(result.ir.native_unknowns("rhino").unwrap().len(), 2);
        let unknown = &result.ir.native_unknowns("rhino").unwrap()[0];
        let retained = &result.source_fidelity.retained_records[0];
        assert_eq!(retained.byte_len, failure.len() as u64);
        assert_eq!(retained.sha256, sha256_hex(&failure));
        assert_eq!(retained.data.as_deref(), Some(failure.as_slice()));
        assert!(unknown.links.is_empty());
        assert!(!result.ir.native_unknowns("rhino").unwrap()[1]
            .links
            .is_empty());
        assert!(result
            .report
            .losses
            .iter()
            .any(|loss| loss.severity >= Severity::Warning));
        assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
    }
}

#[test]
fn nested_brep_crc_warns_without_blocking_object_or_later_point() {
    let mut payload = brep_payload(false);
    let nested_length = i64::from_le_bytes(payload[5..13].try_into().unwrap()) as usize;
    let nested_end = 1 + 12 + nested_length;
    payload[nested_end - 1] ^= 1;
    let brep = object_record(0x10, BREP_CLASS, &payload);
    let point = object_record(1, POINT_CLASS, &point_payload([1.0, 1.0, 1.0]));
    let result = decode(&archive(&[brep, point]));
    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.points.len(), 4);
    assert!(result.report.losses.iter().any(|loss| {
        loss.severity == Severity::Warning
            && loss.message.contains("Brep anonymous CRC mismatch")
            && loss.provenance.is_none()
    }));
    assert!(cadmpeg_ir::validate(&result.ir, result.report.losses.clone()).is_ok());
}

#[test]
fn impossible_outer_object_bound_is_blocking() {
    let object = object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let mut bytes = archive(&[object]);
    let typecode = (0x2000_8070_u32 | 0x0000_8000).to_le_bytes();
    let record = bytes
        .windows(typecode.len())
        .position(|window| window == typecode)
        .expect("object record");
    bytes[record + 4..record + 12].copy_from_slice(&i64::MAX.to_le_bytes());
    RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect_err("impossible outer object bound must block archive decode");
}
