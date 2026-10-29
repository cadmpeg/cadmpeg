// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;
use std::ops::Range;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::UnitVector3;

use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::test_support::test_archive;
use crate::RhinoCodec;

fn model_payload(polyline: bool, height: f64) -> ([u8; 16], Vec<u8>) {
    if polyline {
        (
            test_archive::POLYLINE_CLASS,
            test_archive::polyline_payload(
                &[[0.0, 0.0, 0.0], [0.5, height, 0.0], [1.0, height, 0.0]],
                &[-2.0, 0.5, 6.0],
            ),
        )
    } else {
        (
            test_archive::LINE_CLASS,
            test_archive::line_payload([0.0, height, 0.0], [1.0, height, 0.0], [0.0, 1.0]),
        )
    }
}

fn fixture(
    polyline: bool,
    height: f64,
    bad_support: bool,
) -> (test_archive::PolycurveSource, usize) {
    let mut c2 = if polyline {
        test_archive::polyline_payload(
            &[[0.0, 0.0, 0.0], [0.5, height, 0.0], [1.0, height, 0.0]],
            &[-2.0, 0.5, 6.0],
        )
    } else {
        test_archive::polyline_payload(&[[0.0, height, 0.0], [1.0, height, 0.0]], &[0.0, 1.0])
    };
    let dimension = c2.len() - std::mem::size_of::<i32>();
    c2[dimension..].copy_from_slice(&2_i32.to_le_bytes());
    let mut payload = test_archive::class_wrapper(test_archive::POLYLINE_CLASS, &c2);
    payload.extend(1_i32.to_le_bytes());
    let (class, mut model) = model_payload(polyline, height);
    let fields_len = model.len();
    model.extend(b"unparsed-model-extension");
    payload.extend(test_archive::class_wrapper(class, &model));
    let support = if bad_support {
        vec![0x20]
    } else {
        test_archive::plane_surface_payload()
    };
    payload.extend(test_archive::class_wrapper(
        test_archive::PLANE_SURFACE_CLASS,
        &support,
    ));
    (
        test_archive::polycurve_source(
            &[10.0, 40.0],
            &[(crate::curve_on_surface::CLASS.to_wire(), payload)],
        ),
        fields_len,
    )
}

fn archive(payloads: &[&[u8]]) -> Vec<u8> {
    test_archive::archive(
        &payloads
            .iter()
            .map(|payload| {
                test_archive::object_record(4, crate::curves::POLYCURVE.to_wire(), payload)
            })
            .collect::<Vec<_>>(),
    )
}

fn class_at(data: &[u8], start: usize, end: usize) -> (Range<usize>, usize) {
    let chunk = crate::chunks::chunk_at(data, start, end, ArchiveVersion::V5, false)
        .expect("actual class wrapper");
    let class = crate::objects::parse_class_wrapper(
        data,
        start..chunk.next_offset(),
        ArchiveVersion::V5,
        &mut crate::loss::Diagnostics::new(),
    )
    .expect("actual class-data extent");
    (class.class_data_range, chunk.next_offset())
}

#[test]
fn stored_model_line_and_polyline_source_edits_keep_the_final_component_id() {
    for polyline in [false, true] {
        for height in [0.25, 0.75] {
            let (fixture, fields_len) = fixture(polyline, height, false);
            let bytes = archive(&[&fixture.bytes]);
            let scan = crate::container::scan_owned(bytes.clone()).expect("actual source");
            let range = scan.objects[0]
                .framed()
                .expect("actual object")
                .class_data_range
                .clone();
            let (on_surface, _) = class_at(
                scan.data,
                range.start + fixture.parameter_array.end,
                range.end,
            );
            let (_, next) = class_at(scan.data, on_surface.start, on_surface.end);
            let mut reader =
                BoundedReader::new(scan.data, next, on_surface.end).expect("actual presence field");
            assert_eq!(reader.i32().expect("native C3 presence"), 1);
            let (c3, _) = class_at(scan.data, reader.position(), on_surface.end);
            assert!(c3.start > range.start);
            assert!(c3.start + fields_len < c3.end);
            let decoded = RhinoCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("actual compound model carrier");
            let model = &decoded.ir().model;
            assert_eq!(model.curves.len(), 2);
            assert_eq!(model.procedural_curves.len(), 1);
            assert!(model.surfaces.is_empty());
            let record = &model.procedural_curves[0];
            let ProceduralCurveDefinition::Compound(compound) = record.definition() else {
                panic!("retained compound");
            };
            assert_eq!(
                compound
                    .parameters()
                    .iter()
                    .map(|v| v.get())
                    .collect::<Vec<_>>(),
                [10.0, 40.0]
            );
            assert_eq!(compound.components().len(), 1);
            let child = model
                .curves
                .iter()
                .find(|curve| curve.id == compound.components()[0].component)
                .expect("unchanged actual final model component identity");
            if polyline {
                let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &child.geometry
                else {
                    panic!("actual polyline carrier");
                };
                assert_eq!(curve.degree(), 1);
                assert_eq!(curve.knots().as_slice(), [-2.0, -2.0, 0.5, 6.0, 6.0]);
                assert_eq!(
                    curve
                        .control_points()
                        .into_iter()
                        .map(FinitePoint3::get)
                        .collect::<Vec<_>>(),
                    [
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.5, height, 0.0),
                        Point3::new(1.0, height, 0.0),
                    ]
                );
                assert!(curve.weights().is_none());
                assert!(child.parameter_range.is_none());
            } else {
                let CurveGeometry::Solved(SolvedCurveGeometry::Line(curve)) = &child.geometry
                else {
                    panic!("actual finite line carrier");
                };
                assert_eq!(curve.origin().get(), Point3::new(0.0, height, 0.0));
                assert_eq!(curve.direction(), UnitVector3::X_AXIS);
                assert_eq!(
                    child
                        .parameter_range
                        .expect("native C3 line domain")
                        .endpoints(),
                    [0.0, 1.0]
                );
            }
        }
    }
}

#[test]
fn a_late_support_failure_keeps_the_valid_peer() {
    for polyline in [false, true] {
        let (bad, _) = fixture(polyline, 0.25, true);
        let (valid, _) = fixture(polyline, 0.75, false);
        let bytes = archive(&[&bad.bytes, &valid.bytes]);
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("independent peer survives");
        assert_eq!(decoded.ir().model.curves.len(), 2);
        assert_eq!(decoded.ir().model.procedural_curves.len(), 1);
    }
}
