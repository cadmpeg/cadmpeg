// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::{FRAC_PI_2, SQRT_2, TAU};
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::UnitVector3;

const ARC_POSITION_TOLERANCE: f64 = 1.0e-12;
const ARC_WEIGHT_TOLERANCE: f64 = 1.0e-12;

fn archive() -> Vec<u8> {
    let mut payload = test_archive::arc_payload([0.0, FRAC_PI_2], [10.0, 20.0]);
    payload.extend(b"unparsed-extension");
    test_archive::archive(&[test_archive::object_record(
        4,
        test_archive::ARC_CLASS,
        &payload,
    )])
}

fn assert_quarter_arc(curve: &Curve) {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
        panic!("rational quadratic quarter circle")
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(
        nurbs.knots().as_slice(),
        [10.0, 10.0, 10.0, 20.0, 20.0, 20.0]
    );
    let poles = nurbs.control_points();
    assert_eq!(poles.len(), 3);
    for (actual, expected) in poles.iter().zip([
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(2.0, 2.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]) {
        let actual = actual.get();
        assert!((actual.x - expected.x).abs() < ARC_POSITION_TOLERANCE);
        assert!((actual.y - expected.y).abs() < ARC_POSITION_TOLERANCE);
        assert!((actual.z - expected.z).abs() < ARC_POSITION_TOLERANCE);
    }
    let weights = nurbs.weights().expect("exact rational weights");
    assert_eq!(weights.len(), 3);
    assert_eq!(weights[0].get(), 1.0);
    assert!((weights[1].get() - SQRT_2 / 2.0).abs() < ARC_WEIGHT_TOLERANCE);
    assert_eq!(weights[2].get(), 1.0);
    assert!(curve.parameter_range.is_none());
    assert!(curve.source_object.is_some());
}

#[test]
fn public_arc_decode_keeps_the_curve_before_its_unparsed_extension() {
    let bytes = archive();
    let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
    let range = scan.objects[0]
        .framed()
        .expect("arc record")
        .class_data_range
        .clone();
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source arc");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert_quarter_arc(curve);
    let fields_end = range.start + test_archive::arc_payload([0.0, FRAC_PI_2], [10.0, 20.0]).len();
    assert!(fields_end < range.end);
}

#[test]
fn canonical_native_circle_keeps_its_geometry() {
    let payload = test_archive::arc_payload([0.0, TAU], [0.0, TAU]);
    let bytes = test_archive::archive(&[test_archive::object_record(
        4,
        test_archive::ARC_CLASS,
        &payload,
    )]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source circle");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &curve.geometry else {
        panic!("canonical full circle")
    };
    assert_eq!(circle.center().get(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(circle.radius().get(), 2.0);
    assert_eq!(*circle.frame().axis(), UnitVector3::Z_AXIS);
    assert_eq!(*circle.frame().reference(), UnitVector3::X_AXIS);
    assert!(curve.parameter_range.is_none());
    assert!(curve.source_object.is_some());
}

#[test]
fn an_unformed_arc_keeps_its_valid_sibling() {
    let bytes = test_archive::archive(&[
        test_archive::object_record(
            4,
            test_archive::ARC_CLASS,
            &test_archive::arc_payload([0.0, 0.0], [10.0, 20.0]),
        ),
        test_archive::object_record(
            4,
            test_archive::ARC_CLASS,
            &test_archive::arc_payload([0.0, FRAC_PI_2], [10.0, 20.0]),
        ),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded malformed arc retains its valid sibling");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert_quarter_arc(curve);
}
