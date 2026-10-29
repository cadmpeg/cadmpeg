// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

fn payload() -> Vec<u8> {
    test_archive::polyline_payload(
        &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0]],
        &[-2.0, 0.5, 6.0],
    )
}

fn archive() -> Vec<u8> {
    let mut payload = payload();
    payload.extend(b"unparsed-extension");
    test_archive::archive(&[test_archive::object_record(
        4,
        test_archive::POLYLINE_CLASS,
        &payload,
    )])
}

fn assert_polyline(curve: &Curve) {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
        panic!("degree-one polyline carrier")
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [-2.0, -2.0, 0.5, 6.0, 6.0]);
    assert_eq!(
        nurbs
            .control_points()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(3.0, 2.0, 0.0)
        ]
    );
    assert!(nurbs.weights().is_none());
    assert!(curve.parameter_range.is_none());
    assert!(curve.source_object.is_some());
}

#[test]
fn public_polyline_decode_keeps_the_curve_before_its_unparsed_extension() {
    let bytes = archive();
    let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
    let range = scan.objects[0]
        .framed()
        .expect("polyline record")
        .class_data_range
        .clone();
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source polyline");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert_polyline(curve);
    let fields_end = range.start + payload().len();
    assert!(fields_end < range.end);
}

#[test]
fn a_polyline_with_mismatched_counts_keeps_its_sibling() {
    let malformed = test_archive::polyline_payload(
        &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 2.0, 0.0]],
        &[-2.0, 0.5],
    );
    let bytes = test_archive::archive(&[
        test_archive::object_record(4, test_archive::POLYLINE_CLASS, &malformed),
        test_archive::object_record(4, test_archive::POLYLINE_CLASS, &payload()),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded count mismatch retains its valid sibling");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert_polyline(curve);
}
