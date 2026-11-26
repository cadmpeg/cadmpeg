// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_test_support::wire;

const EPS_TOPOLOGY_TOLERANCE: f64 = 1.0e-12;

use std::fmt::Write;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::report::Severity;
use sha2::{Digest, Sha256};

use crate::layout::compressed_buffer_prologue as compressed;
use crate::layout::long_chunk_header_wide as long_wide;
use crate::test_support::test_archive::{
    arc_payload, archive, archive_unit, archive_version, archive_writer, brep_payload,
    class_wrapper, line_payload, mesh_payload, object_record, point_cloud_payload, point_payload,
    polycurve_payload, polyline_payload, singular_seam_brep_payload, ARC_CLASS, BREP_CLASS,
    EXTRUSION_CLASS, LINE_CLASS, POINT_CLOUD_CLASS, POLYCURVE_CLASS, POLYLINE_CLASS,
};
use crate::test_support::test_dump::{
    MESH_CLASS, NURBS_CURVE_CLASS, POINT_CLASS, REV_SURFACE_CLASS, SUBD_CLASS,
};
use crate::RhinoCodec;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

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
    assert_eq!(
        &record[long_wide::LEN..long_wide::LEN + 4],
        &0x8200_0071_u32.to_le_bytes()
    );
    assert_eq!(
        &record[record.len() - 16..record.len() - 12],
        &0x8200_007f_u32.to_le_bytes()
    );

    let result = decode(&archive(&[record]));

    assert_eq!(
        result.ir().geometry.points.len(),
        1,
        "{:?}",
        result.report()
    );
    assert!(result.report().geometry_transferred());
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

    assert_eq!(
        result.ir().geometry.points.len(),
        1,
        "{:?}",
        result.report()
    );
    assert_eq!(
        result.ir().geometry.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    assert_eq!(result.ir().geometry.curves.len(), 1);
    let Some(SolvedCurveGeometry::Nurbs(curve)) = result.ir().geometry.curves[0].geometry.solved()
    else {
        panic!("bounded line must decode to an exact NURBS carrier");
    };
    assert_eq!(curve.degree(), 1);
    assert_eq!(curve.knots().as_slice(), [3.0, 3.0, 7.0, 7.0]);
    assert_eq!(
        curve.control_points(),
        vec![
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0),
            cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
        ]
    );
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")
            .len(),
        2
    );
    assert!(result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[0]
        .links
        .iter()
        .any(|link| link.as_str() == result.ir().geometry.bodies[0].id.as_str()));
    assert!(result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[1]
        .links
        .iter()
        .any(|link| link.as_str() == result.ir().geometry.curves[0].id.as_str()));
    assert!(result.report().geometry_transferred());
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn future_and_semantically_invalid_objects_are_atomic_and_later_point_recovers() {
    for malformed in [
        object_record(1, POINT_CLASS, &[0x20]),
        object_record(1, POINT_CLASS, &point_payload([f64::NAN, 0.0, 0.0])),
    ] {
        let valid = object_record(1, POINT_CLASS, &point_payload([4.0, 5.0, 6.0]));
        let result = decode(&archive(&[malformed, valid]));

        assert_eq!(
            result.ir().geometry.points.len(),
            1,
            "{:?}",
            result.report()
        );
        assert_eq!(
            result.ir().geometry.points[0].position().get(),
            cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
        );
        assert!(result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links
            .is_empty());
        assert!(!result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[1]
            .links
            .is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity != Severity::Info));
        assert!(cadmpeg_ir::validate::validate_neutral(
            result.ir(),
            result.report().losses.clone()
        )
        .expect("validate the neutral model")
        .is_ok());
    }
}

#[test]
fn retention_caps_store_only_complete_records_with_exact_hashes() {
    let large = object_record(1, [0; 16], &[0x55; 64]);
    let point = object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let bytes = archive(&[large.clone(), point.clone()]);
    let scan = crate::container::scan_owned(bytes).expect("complete archive scan");
    let result = EditableDecodeResult::from(crate::decode::with_expand(&scan, |expand| {
        let mut context =
            crate::decode::DecodeContext::new(&scan, expand).expect("test transaction");
        context
            .set_retention_limits(point.len(), point.len())
            .expect("test retention limits");
        crate::decode::seal_for_test(context.commit().expect("test decode commit"), false)
    }));

    let retained = &result.source_fidelity().retained_records();
    assert_eq!(
        retained
            .values()
            .next()
            .expect("retained record")
            .byte_len(),
        cadmpeg_core::decode::u64_from_index(large.len())
    );
    assert_eq!(
        retained.values().next().expect("retained record").sha256(),
        sha256_hex(&large)
    );
    assert_eq!(
        retained.values().next().expect("retained record").data(),
        None
    );
    assert_eq!(
        retained
            .values()
            .nth(1)
            .expect("retained record")
            .byte_len(),
        cadmpeg_core::decode::u64_from_index(point.len())
    );
    assert_eq!(
        retained.values().nth(1).expect("retained record").sha256(),
        sha256_hex(&point)
    );
    assert_eq!(
        retained.values().nth(1).expect("retained record").data(),
        Some(point.as_slice())
    );

    let two_points = archive(&[point.clone(), point.clone()]);
    let scan = crate::container::scan_owned(two_points).expect("complete archive scan");
    let result = EditableDecodeResult::from(crate::decode::with_expand(&scan, |expand| {
        let mut context =
            crate::decode::DecodeContext::new(&scan, expand).expect("test transaction");
        context
            .set_retention_limits(point.len(), point.len())
            .expect("test retention limits");
        crate::decode::seal_for_test(context.commit().expect("test decode commit"), false)
    }));
    assert_eq!(
        result
            .source_fidelity()
            .retained_records()
            .values()
            .next()
            .expect("retained record")
            .data(),
        Some(point.as_slice())
    );
    assert_eq!(
        result
            .source_fidelity()
            .retained_records()
            .values()
            .nth(1)
            .expect("retained record")
            .data(),
        None
    );
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
        first
            .ir()
            .to_canonical_json()
            .expect("required invariant")
            .as_bytes(),
        second
            .ir()
            .to_canonical_json()
            .expect("required invariant")
            .as_bytes()
    );
    assert_eq!(
        serde_json::to_vec(first.report()).expect("required invariant"),
        serde_json::to_vec(second.report()).expect("required invariant")
    );
}

#[test]
fn subd_complete_object_commits_across_supported_archive_bands() {
    for (version, archive_band) in [
        ("50", crate::chunks::ArchiveVersion::V5),
        ("60", crate::chunks::ArchiveVersion::V6),
        ("70", crate::chunks::ArchiveVersion::V7),
        ("80", crate::chunks::ArchiveVersion::V8),
    ] {
        let object = object_record(
            0x0004_0000,
            SUBD_CLASS,
            &crate::subd::tests::quad_payload(archive_band),
        );
        let result = decode(&archive_version(version, &[object]));
        assert_eq!(result.ir().geometry.subds.len(), 1, "archive {version}");
        assert_eq!(
            result.ir().geometry.subds[0].cage.faces()[0]
                .edge_uses()
                .count(),
            4
        );
        assert!(!result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links
            .is_empty());
        assert!(cadmpeg_ir::validate::validate_neutral(
            result.ir(),
            result.report().losses.clone()
        )
        .expect("validate the neutral model")
        .is_ok());
    }
}

#[test]
fn polycurve_nested_past_the_curve_depth_cap_fails_one_object() {
    let mut nested = line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]);
    let mut class = LINE_CLASS;
    for _ in 0..40 {
        nested = polycurve_payload(&[0.0, 1.0], &[(class, nested)]);
        class = POLYCURVE_CLASS;
    }
    let result = decode(&archive(&[
        object_record(4, POLYCURVE_CLASS, &nested),
        object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
    ]));

    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(result.ir().geometry.curves.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("curve recursion limit exceeded")));
}

#[test]
fn revolution_profile_over_the_join_degree_cap_fails_one_object() {
    // Order 66 is degree 65, one above `MAX_JOIN_DEGREE`.
    let order = 66_i32;
    let mut nurbs = vec![0x10];
    for value in [3, 0, order, order, 0, 0] {
        nurbs.extend(value.to_le_bytes());
    }
    nurbs.extend([0_u8; 48]);
    nurbs.extend((2 * order - 2).to_le_bytes());
    for index in 0..2 * order - 2 {
        let knot = if index < order - 1 { 1.0_f64 } else { 2.0 };
        nurbs.extend(knot.to_le_bytes());
    }
    nurbs.extend(order.to_le_bytes());
    for index in 0..order {
        for value in [f64::from(index), 1.0, 0.0] {
            nurbs.extend(value.to_le_bytes());
        }
    }
    let profile = polycurve_payload(
        &[0.0, 1.0, 2.0],
        &[
            (
                LINE_CLASS,
                line_payload([0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0]),
            ),
            (NURBS_CURVE_CLASS, nurbs),
        ],
    );
    let mut revolution = vec![0x20];
    for value in [
        0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, -10.0, -10.0, -10.0, 10.0, 10.0, 10.0,
    ] {
        revolution.extend(f64::to_le_bytes(value));
    }
    revolution.extend(0_i32.to_le_bytes());
    revolution.push(1);
    revolution.extend(class_wrapper(POLYCURVE_CLASS, &profile));
    let result = decode(&archive(&[
        object_record(8, REV_SURFACE_CLASS, &revolution),
        object_record(1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
    ]));

    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(result.ir().geometry.surfaces.iter().all(|surface| matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
    )));
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("MAX_JOIN_DEGREE")),
        "{:#?}",
        result.report().losses
    );
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

    assert_eq!(result.ir().geometry.points.len(), 3);
    assert_eq!(
        result.ir().geometry.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
    assert_eq!(
        result.ir().geometry.points[2].position().get(),
        cadmpeg_ir::math::Point3::new(7.0, 8.0, 9.0)
    );
    assert_eq!(result.ir().geometry.curves.len(), 7);
    let line = result
        .ir()
        .geometry
        .curves
        .iter()
        .find(|curve| {
            matches!(
                &curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
                    if nurbs.knots().as_slice() == [-1.0, -1.0, 3.0, 3.0]
            )
        })
        .expect("bounded line");
    let Some(SolvedCurveGeometry::Nurbs(line)) = line.geometry.solved() else {
        panic!("line carrier");
    };
    assert_eq!(line.knots().as_slice(), [-1.0, -1.0, 3.0, 3.0]);
    let polyline = result
        .ir()
        .geometry
        .curves
        .iter()
        .find(|curve| {
            matches!(
                &curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
                    if nurbs.knots().as_slice() == [2.0, 2.0, 3.5, 9.0, 9.0]
            )
        })
        .expect("polyline");
    let Some(SolvedCurveGeometry::Nurbs(polyline)) = polyline.geometry.solved() else {
        panic!("polyline carrier");
    };
    assert_eq!(polyline.knots().as_slice(), [2.0, 2.0, 3.5, 9.0, 9.0]);
    assert!(result.ir().geometry.procedural_curves.is_empty());
    let composite = |range: [f64; 2]| {
        let curve = result
            .ir()
            .geometry
            .curves
            .iter()
            .find(|curve| {
                matches!(
                    curve.geometry,
                    CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
                ) && curve
                    .parameter_range
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
                    == Some(range)
            })
            .unwrap_or_else(|| panic!("composite over {range:?}"));
        let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
            &curve.geometry
        else {
            unreachable!("a composite curve");
        };
        segments
            .iter()
            .map(|segment| {
                assert!(segment.same_sense);
                result
                    .ir()
                    .geometry
                    .curves
                    .iter()
                    .find(|curve| curve.id == segment.curve)
                    .expect("the segment curve")
            })
            .collect::<Vec<_>>()
    };
    let root = composite([0.0, 5.0]);
    assert_eq!(root.len(), 2);
    assert!(root[0].id.as_str().contains("component-0"));
    assert!(root[1].id.as_str().contains("component-1"));
    let Some(SolvedCurveGeometry::Nurbs(first)) = root[0].geometry.solved() else {
        panic!("first segment carrier");
    };
    assert_eq!(first.knots().as_slice(), [0.0, 0.0, 2.0, 2.0]);
    let inner = composite([2.0, 5.0]);
    let [inner_line] = inner.as_slice() else {
        panic!("one inner segment");
    };
    let Some(SolvedCurveGeometry::Nurbs(inner_line)) = inner_line.geometry.solved() else {
        panic!("inner segment carrier");
    };
    assert_eq!(inner_line.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")
            .len(),
        6
    );
    assert!(result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")
        .iter()
        .all(|record| !record.links.is_empty()));
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
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
                result.ir().model.tessellations.len(),
                1,
                "major {major} minor {minor}: {:?}",
                result.report()
            );
            let mesh = &result.ir().model.tessellations[0];
            assert_eq!(mesh.vertices().len(), 4);
            assert_eq!(mesh.triangles(), vec![[0, 1, 2], [0, 2, 3], [0, 3, 1]]);
            assert_eq!(mesh.vertex_normals().len(), 4);
            assert!(mesh
                .channels()
                .iter()
                .any(|channel| channel.kind() == 0x5248_0001));
            assert!(mesh
                .channels()
                .iter()
                .any(|channel| channel.kind() == 0x5248_0002));
            if major == 3 && minor >= 3 {
                assert!(mesh
                    .channels()
                    .iter()
                    .any(|channel| channel.kind() == 0x5248_0003));
            }
            assert_eq!(
                mesh.vertices()[2],
                cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0)
            );
            assert!(cadmpeg_ir::validate::validate_neutral(
                result.ir(),
                result.report().losses.clone()
            )
            .expect("validate the neutral model")
            .is_ok());
        }
    }
}

#[test]
fn required_mesh_channel_failure_is_atomic_and_optional_crc_is_recoverable() {
    let bad_mesh = object_record(0x20, MESH_CLASS, &mesh_payload(3, 5, true, false));
    let point = object_record(1, POINT_CLASS, &point_payload([8.0, 9.0, 10.0]));
    let result = decode(&archive(&[bad_mesh, point]));
    assert!(result.ir().model.tessellations.is_empty());
    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[0]
        .links
        .is_empty());
    assert!(!result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[1]
        .links
        .is_empty());
    let failure = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.severity == Severity::Error)
        .expect("mesh failure loss");
    assert_eq!(
        failure.code,
        crate::loss::RhinoLossCode::ObjectFramingUndecodable.kind()
    );
    assert!(failure.message.contains(
        "1 framed object record(s) for class 4ed7d4e4-e947-11d3-bfe5-0010830122f0 could not be decoded"
    ));
    let provenance = failure.provenance.as_ref().expect("failure provenance");
    assert_eq!(wire::field::<String>(&provenance, "format"), "rhino");
    assert!(provenance
        .tag
        .as_deref()
        .is_some_and(|tag| tag.starts_with("OBJECT_RECORD/class=4ed7d4e4")));

    let mut optional_crc = mesh_payload(3, 5, false, false);
    let vertex_buffer = 1 + 8 + 64 + 16 + 64 + 4 + 5 + 4 + 8;
    let normal_buffer = vertex_buffer + 4 + 4 + 1 + 48;
    optional_crc[normal_buffer + compressed::CRC32..normal_buffer + compressed::METHOD]
        .copy_from_slice(&0_u32.to_le_bytes());
    let result = decode(&archive(&[object_record(0x20, MESH_CLASS, &optional_crc)]));
    assert_eq!(
        result.ir().model.tessellations.len(),
        1,
        "{:?}",
        result.report()
    );
    assert!(result.ir().model.tessellations[0]
        .vertex_normals()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == Severity::Error && loss.message.contains("normals")));
}

/// `ObjectFramingUndecodable` carries severity `Error` and pins its strict
/// floor at `Warning`, above the tolerable floor of its `DecodeDiagnostic`
/// taxonomy, so the loss rejects. Strict decode therefore refuses the archive,
/// and it refuses with the policy class that names the loss: the container
/// parses, one record payload does not, and salvage mode keeps the result.
#[test]
fn strict_decode_refuses_an_undecodable_framed_object_record() {
    let bad_mesh = object_record(0x20, MESH_CLASS, &mesh_payload(3, 5, true, false));
    let point = object_record(1, POINT_CLASS, &point_payload([8.0, 9.0, 10.0]));
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;

    let error = RhinoCodec
        .decode(&mut Cursor::new(archive(&[bad_mesh, point])), &options)
        .expect_err("strict mode refuses an undecodable framed object record");

    let cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } = &error else {
        panic!("a strict refusal is a policy class, not a container defect: {error:?}");
    };
    assert_eq!(
        rejection.loss().code,
        crate::loss::RhinoLossCode::ObjectFramingUndecodable.kind()
    );
    assert!(
        rejection
            .loss()
            .message
            .starts_with("1 framed object record(s)"),
        "the refusal carries the loss message alone: {}",
        rejection.loss().message
    );
}

/// A framed object record the decoder gives up also charges
/// `record.unreadable` naming its retained source record: a pinned decode
/// refuses with that charge in its report, and a default decode admits the
/// document and reports both the codec's loss and the charge.
#[test]
fn a_given_up_framed_object_record_is_charged_as_unreadable() {
    let bad_mesh = object_record(0x20, MESH_CLASS, &mesh_payload(3, 5, true, false));
    let point = object_record(1, POINT_CLASS, &point_payload([8.0, 9.0, 10.0]));
    let bytes = archive(&[bad_mesh, point]);
    let charged = |losses: &[cadmpeg_ir::report::loss::LossNote]| {
        losses.iter().any(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss.severity == Severity::Error
                && loss
                    .message
                    .split_whitespace()
                    .any(|token| token == "rhino:object:record#000000")
        })
    };
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;

    let error = RhinoCodec
        .decode(&mut Cursor::new(bytes.as_slice()), &options)
        .expect_err("a pinned decode refuses a given-up framed object record");
    let cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } = &error else {
        panic!("a strict refusal is a policy class, not a container defect: {error:?}");
    };
    assert!(charged(&rejection.report().losses), "{error}");

    let result = decode(&bytes);
    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::RhinoLossCode::ObjectFramingUndecodable.kind()));
    assert!(charged(&result.report().losses));
}

#[test]
fn serialized_extrusion_versions_caps_holes_and_cache_dispatch_atomically() {
    for minor in 0..=3 {
        let payload = crate::extrusion::tests::archive_payload(
            minor,
            [minor != 2, minor == 2],
            false,
            minor == 3,
        );
        let result = decode(&archive(&[object_record(0x10, EXTRUSION_CLASS, &payload)]));
        assert_eq!(
            result.ir().geometry.procedural_surfaces.len(),
            1,
            "minor {minor}: {:?}",
            result.report()
        );
        let expected_caps = if minor < 2 { 2 } else { 1 };
        assert_eq!(result.ir().geometry.faces.len(), expected_caps);
        assert_eq!(
            result.ir().model.tessellations.len(),
            usize::from(minor == 3)
        );
        assert!(!result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links
            .is_empty());
        assert!(cadmpeg_ir::validate::validate_neutral(
            result.ir(),
            result.report().losses.clone()
        )
        .expect("validate the neutral model")
        .is_ok());
    }

    let payload = crate::extrusion::tests::archive_payload(2, [true, true], true, false);
    let result = decode(&archive(&[object_record(0x10, EXTRUSION_CLASS, &payload)]));
    assert_eq!(result.ir().geometry.procedural_surfaces.len(), 2);
    assert_eq!(result.ir().geometry.faces.len(), 2);
    assert_eq!(result.ir().geometry.loops.len(), 4);
    assert_eq!(result.ir().geometry.pcurves.len(), 4);
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn invalid_extrusion_profile_is_one_unknown_surface_and_later_point_recovers() {
    let mut malformed = crate::extrusion::tests::archive_payload(2, [true, false], false, false);
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
    assert!(result.ir().geometry.procedural_surfaces.is_empty());
    assert!(result.ir().geometry.faces.is_empty());
    assert!(result.ir().geometry.pcurves.is_empty());
    assert_eq!(
        result
            .ir()
            .geometry
            .surfaces
            .iter()
            .filter(|surface| matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Unknown { .. }
                )
            ))
            .count(),
        1
    );
    assert_eq!(result.ir().geometry.points.len(), 1);
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn serialized_brep_l3_commits_connected_topology_pcurves_and_scaled_tolerances() {
    let payload = brep_payload(false);
    assert_eq!(payload[13], 0x10);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("build the decode context");
    crate::brep::parse(
        &ctx,
        &payload,
        0..payload.len(),
        crate::chunks::ArchiveVersion::V5,
        None,
        &[],
    )
    .expect("direct Brep fixture parse");
    let result = decode(&archive_unit(
        3,
        &[object_record(0x10, BREP_CLASS, &payload)],
    ));
    let geometry = &result.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1, "{:?}", result.report());
    assert_eq!(geometry.regions.len(), 1);
    assert_eq!(geometry.shells.len(), 1);
    assert_eq!(geometry.faces.len(), 1);
    assert_eq!(geometry.loops.len(), 1);
    assert_eq!(geometry.coedges.len(), 3);
    assert_eq!(geometry.edges.len(), 3);
    assert_eq!(geometry.vertices.len(), 3);
    assert_eq!(geometry.pcurves.len(), 3);
    assert_eq!(geometry.curves.len(), 3);
    assert_eq!(geometry.surfaces.len(), 1);
    let body = &geometry.bodies[0];
    let region = &geometry.regions[0];
    let shell = &geometry.shells[0];
    let face = &geometry.faces[0];
    let loop_record = &geometry.loops[0];
    assert_eq!(region.body, body.id);
    assert_eq!(
        region.stated_shells().cloned().collect::<Vec<_>>(),
        vec![shell.id.clone()]
    );
    assert_eq!(shell.region, region.id);
    assert_eq!(face.shell, shell.id);
    assert_eq!(
        face.loops.boundary().expect("a complete face"),
        vec![loop_record.id.clone()]
    );
    assert_eq!(loop_record.face, face.id);
    assert!(geometry
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()
            && geometry.edges.iter().any(|edge| edge.id == coedge.edge)));
    assert!(geometry.edges.iter().all(|edge| edge.curve().is_some()
        && edge
            .tolerance
            .is_some_and(|tolerance| (tolerance.get() - 0.3).abs() < EPS_TOPOLOGY_TOLERANCE)));
    assert!(geometry.vertices.iter().all(|vertex| vertex
        .tolerance
        .is_some_and(|tolerance| (tolerance.get() - 0.2).abs() < EPS_TOPOLOGY_TOLERANCE)));
    assert!(geometry.pcurves.iter().all(|pcurve| pcurve
        .fit_tolerance()
        .map(cadmpeg_ir::geometry::legacy_cache::FitTolerance::get)
        == Some(0.04)));
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")
            .len(),
        1
    );
    assert!(result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[0]
        .links
        .iter()
        .any(|link| link.as_str() == body.id.as_str()));
    assert!(result.report().geometry_transferred());
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::ObjectRecordCensus.kind()
        && loss.message.contains("decoded 1/1 Rhino object records")));
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn serialized_singular_seam_ring_uses_directed_trim_vertices() {
    let valid = decode(&archive(&[object_record(
        0x10,
        BREP_CLASS,
        &singular_seam_brep_payload(false),
    )]));
    let geometry = &valid.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1, "{:?}", valid.report());
    assert_eq!(geometry.faces.len(), 1);
    assert_eq!(geometry.loops.len(), 1);
    assert_eq!(geometry.coedges.len(), 4);
    assert_eq!(geometry.edges.len(), 3);
    assert_eq!(geometry.vertices.len(), 2);
    assert_eq!(geometry.pcurves.len(), 4);
    assert_eq!(
        geometry.coedges[3].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(valid.ir(), valid.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );

    let malformed = decode(&archive(&[object_record(
        0x10,
        BREP_CLASS,
        &singular_seam_brep_payload(true),
    )]));
    assert!(malformed.ir().geometry.faces.is_empty());
    assert!(malformed.ir().geometry.loops.is_empty());
    assert!(malformed.ir().geometry.coedges.is_empty());
    assert!(malformed.report().losses.iter().any(|loss| {
        loss.severity == Severity::Warning
            && loss.message.contains("Brep topology fallback")
            && loss.message.contains("loop ring is discontinuous")
    }));
    assert!(cadmpeg_ir::validate::validate_neutral(
        malformed.ir(),
        malformed.report().losses.clone()
    )
    .expect("validate the neutral model")
    .is_ok());
}

#[test]
fn semantic_invalid_brep_keeps_only_free_c3_surface_and_later_point() {
    let point = object_record(1, POINT_CLASS, &point_payload([9.0, 8.0, 7.0]));
    let result = decode(&archive(&[
        object_record(0x10, BREP_CLASS, &brep_payload(true)),
        point,
    ]));
    let geometry = &result.ir().geometry;
    assert!(geometry.faces.is_empty());
    assert!(geometry.loops.is_empty());
    assert!(geometry.coedges.is_empty());
    assert!(geometry.edges.is_empty());
    assert!(geometry.pcurves.is_empty());
    assert_eq!(geometry.curves.len(), 3);
    assert_eq!(geometry.surfaces.len(), 1);
    assert_eq!(geometry.points.len(), 1);
    assert!(!result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[0]
        .links
        .is_empty());
    assert!(!result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[1]
        .links
        .is_empty());
    assert!(
        result.report().losses.iter().any(|loss| {
            loss.severity == Severity::Warning && loss.message.contains("Brep topology fallback")
        }),
        "{:?}",
        result.report()
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn archive_failure_recovery_matrix_preserves_exact_unknown_records() {
    let mut malformed_zlib = mesh_payload(3, 0, false, false);
    let common_bytes = 1 + 8 + 64 + 16 + 64 + 4 + 5 + 4 + 8;
    malformed_zlib[common_bytes + compressed::METHOD] = 1;

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
        let result = EditableDecodeResult::from(decode(&archive(&[failure.clone(), point])));
        assert_eq!(
            result.ir().geometry.points.len(),
            1,
            "{:?}",
            result.report()
        );
        assert_eq!(
            result
                .ir()
                .native_unknowns("rhino")
                .expect("required invariant")
                .len(),
            2
        );
        let unknown = &result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0];
        let retained = &result
            .source_fidelity()
            .retained_records()
            .values()
            .next()
            .expect("retained record");
        assert_eq!(
            retained.byte_len(),
            cadmpeg_core::decode::u64_from_index(failure.len())
        );
        assert_eq!(retained.sha256(), sha256_hex(&failure));
        assert_eq!(retained.data(), Some(failure.as_slice()));
        assert!(unknown.links.is_empty());
        assert!(!result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[1]
            .links
            .is_empty());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.severity >= Severity::Warning));
        assert!(cadmpeg_ir::validate::validate_neutral(
            result.ir(),
            result.report().losses.clone()
        )
        .expect("validate the neutral model")
        .is_ok());
    }
}

#[test]
fn nested_brep_crc_warns_without_blocking_object_or_later_point() {
    let mut payload = brep_payload(false);
    let nested_length = usize::try_from(i64::from_le_bytes(
        payload[(1 + long_wide::DECLARED_LENGTH)..=long_wide::LEN]
            .try_into()
            .expect("required invariant"),
    ))
    .expect("fixture value fits usize");
    let nested_end = 1 + long_wide::LEN + nested_length;
    payload[nested_end - 1] ^= 1;
    let brep = object_record(0x10, BREP_CLASS, &payload);
    let point = object_record(1, POINT_CLASS, &point_payload([1.0, 1.0, 1.0]));
    let result = decode(&archive(&[brep, point]));
    assert_eq!(result.ir().geometry.bodies.len(), 2);
    assert_eq!(result.ir().geometry.faces.len(), 1);
    assert_eq!(result.ir().geometry.points.len(), 4);
    assert!(result.report().losses.iter().any(|loss| {
        loss.severity == Severity::Error
            && loss.message.contains("Brep anonymous CRC mismatch")
            && loss.provenance.is_none()
    }));
    assert!(
        cadmpeg_ir::validate::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
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
    bytes[record + long_wide::DECLARED_LENGTH..record + long_wide::LEN]
        .copy_from_slice(&i64::MAX.to_le_bytes());
    RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect_err("impossible outer object bound must block archive decode");
}
