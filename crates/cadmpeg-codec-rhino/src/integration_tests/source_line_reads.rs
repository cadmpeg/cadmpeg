// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::units::UnitVector3;

fn archive() -> Vec<u8> {
    let mut payload = test_archive::line_payload([1.0, -4.0, 3.0], [1.0, 4.0, 3.0], [-2.0, 6.0]);
    payload.extend(b"unparsed-extension");
    test_archive::archive(&[test_archive::object_record(
        4,
        test_archive::LINE_CLASS,
        &payload,
    )])
}

#[test]
fn public_line_decode_keeps_the_curve_before_its_unparsed_extension() {
    let bytes = archive();
    let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
    let range = scan.objects[0]
        .framed()
        .expect("line record")
        .class_data_range
        .clone();
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source line");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &curve.geometry else {
        panic!("unit-speed line projection")
    };
    assert_eq!(line.origin().get(), Point3::new(1.0, -2.0, 3.0));
    assert_eq!(line.direction(), UnitVector3::Y_AXIS);
    assert_eq!(
        curve.parameter_range.expect("native domain").endpoints(),
        [-2.0, 6.0]
    );
    assert!(curve.source_object.is_some());
    let fields_end = range.start
        + test_archive::line_payload([1.0, -4.0, 3.0], [1.0, 4.0, 3.0], [-2.0, 6.0]).len();
    assert!(fields_end < range.end);
}

#[test]
fn nonunit_native_line_keeps_its_exact_parameterization() {
    let payload = test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 5.0]);
    let bytes = test_archive::archive(&[test_archive::object_record(
        4,
        test_archive::LINE_CLASS,
        &payload,
    )]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source line");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert!(curve.parameter_range.is_none());
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
        panic!("nonunit native line parameterization")
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        nurbs
            .control_points()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]
    );
    assert!(curve.source_object.is_some());
}

#[test]
fn an_unformed_line_keeps_its_valid_sibling() {
    let bytes = test_archive::archive(&[
        test_archive::object_record(
            4,
            test_archive::LINE_CLASS,
            &test_archive::line_payload([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 2.0]),
        ),
        test_archive::object_record(
            4,
            test_archive::LINE_CLASS,
            &test_archive::line_payload([1.0, -4.0, 3.0], [1.0, 4.0, 3.0], [-2.0, 6.0]),
        ),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded malformed line retains its valid sibling");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &curve.geometry else {
        panic!("valid unit-speed line sibling")
    };
    assert_eq!(line.origin().get(), Point3::new(1.0, -2.0, 3.0));
    assert_eq!(line.direction(), UnitVector3::Y_AXIS);
    assert_eq!(
        curve.parameter_range.expect("native domain").endpoints(),
        [-2.0, 6.0]
    );
}
