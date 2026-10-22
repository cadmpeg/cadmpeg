// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::{test_archive, test_dump};
use crate::RhinoCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

fn archive() -> Vec<u8> {
    let mut fixture = test_archive::rational_nurbs_curve_source(0x11, [2.0, 4.0]);
    fixture.bytes.extend(b"unparsed-extension");
    test_archive::archive(&[test_archive::object_record(
        4,
        test_dump::NURBS_CURVE_CLASS,
        &fixture.bytes,
    )])
}

fn assert_nurbs(curve: &Curve) {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &curve.geometry else {
        panic!("native NURBS class stays NURBS")
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        nurbs
            .control_points()
            .into_iter()
            .map(FinitePoint3::get)
            .collect::<Vec<_>>(),
        [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)]
    );
    assert_eq!(
        nurbs
            .weights()
            .expect("rational lane")
            .into_iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        [2.0, 4.0]
    );
    assert!(!nurbs.periodic());
    assert!(curve.parameter_range.is_none());
    assert!(curve.source_object.is_some());
}

#[test]
fn public_nurbs_decode_installs_exact_fields_without_crediting_reserved_bounds_or_suffix() {
    for version in [0x10, 0x11] {
        let mut fixture = test_archive::rational_nurbs_curve_source(version, [2.0, 4.0]);
        let fields_len = fixture.bytes.len();
        fixture.bytes.extend(b"unparsed-extension");
        let bytes = test_archive::archive(&[test_archive::object_record(
            4,
            test_dump::NURBS_CURVE_CLASS,
            &fixture.bytes,
        )]);
        let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
        let range = scan.objects[0]
            .framed()
            .expect("NURBS record")
            .class_data_range();
        let result = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("source NURBS");
        assert_eq!(result.ir().model.curves.len(), 1);
        let curve = &result.ir().model.curves[0];
        assert_nurbs(curve);
        let layout = &result.source_fidelity().record_layout()[curve.id.as_str()];
        assert_eq!(
            layout.offset(),
            u64::try_from(range.start).expect("small start")
        );
        assert_eq!(
            layout.byte_len(),
            u64::try_from(range.len()).expect("small extent")
        );
        assert_eq!(
            layout.reads()["file"],
            [
                u64::try_from(range.start).expect("small start")
                    ..u64::try_from(range.start + fixture.reserved_bounds.start)
                        .expect("small bounds start"),
                u64::try_from(range.start + fixture.reserved_bounds.end).expect("small bounds end")
                    ..u64::try_from(range.start + fields_len).expect("small fields end")
            ]
        );
        assert!(range.start + fields_len < range.end);
    }
}

#[test]
fn detached_nurbs_output_keeps_the_same_curve_without_source_captures() {
    let bytes = archive();
    let source = RhinoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("source NURBS");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let detached = RhinoCodec
        .decode_with_context(&ctx, View::over_retained(&bytes))
        .expect("pure authored-output parse");
    assert_eq!(detached.ir().model.curves, source.ir().model.curves);
    assert!(detached.source_fidelity().record_layout().is_empty());
    assert!(ctx
        .take_record_captures()
        .expect("no fabricated source captures")
        .is_empty());
}

#[test]
fn a_foreign_registered_nurbs_view_cannot_enter_the_producer_scope() {
    let bytes = archive();
    let first_arena = DecodeArena::new();
    let (_first, source) =
        DecodeContext::from_root_bytes(&bytes, &first_arena, &DecodePolicy::default())
            .expect("first session");
    let second_arena = DecodeArena::new();
    let (second, _) =
        DecodeContext::from_root_bytes(b"other", &second_arena, &DecodePolicy::default())
            .expect("second session");
    let failure = RhinoCodec
        .decode_with_context(&second, source)
        .expect_err("foreign source cannot enter the NURBS record scope");
    assert!(failure
        .to_string()
        .contains("another or detached decode session"));
    assert!(second
        .take_record_captures()
        .expect("no foreign capture committed")
        .is_empty());
}

#[test]
fn an_invalid_nurbs_weight_keeps_no_capture_and_its_valid_sibling_survives() {
    let malformed = test_archive::rational_nurbs_curve_source(0x10, [2.0, 0.0]);
    let valid = test_archive::rational_nurbs_curve_source(0x10, [2.0, 4.0]);
    let bytes = test_archive::archive(&[
        test_archive::object_record(4, test_dump::NURBS_CURVE_CLASS, &malformed.bytes),
        test_archive::object_record(4, test_dump::NURBS_CURVE_CLASS, &valid.bytes),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded invalid weight retains its valid sibling");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert_nurbs(curve);
    assert_eq!(result.source_fidelity().record_layout().len(), 1);
    assert!(result
        .source_fidelity()
        .record_layout()
        .contains_key(curve.id.as_str()));
}

fn order_one_nurbs_payload() -> Vec<u8> {
    let mut bytes = vec![0x10];
    for value in [3_i32, 0, 1, 2, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(2.0_f64.to_le_bytes());
    bytes.extend(2_i32.to_le_bytes());
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

#[test]
fn an_order_one_nurbs_record_is_rejected_without_losing_its_valid_peer() {
    let valid = test_archive::rational_nurbs_curve_source(0x10, [2.0, 4.0]);
    let bytes = test_archive::archive(&[
        test_archive::object_record(4, test_dump::NURBS_CURVE_CLASS, &order_one_nurbs_payload()),
        test_archive::object_record(4, test_dump::NURBS_CURVE_CLASS, &valid.bytes),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("invalid order is bounded to its record");
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    assert_nurbs(curve);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ObjectDecodeDiagnostic.kind()
            && loss.message.contains("curve order")
    }));
    assert_eq!(result.source_fidelity().record_layout().len(), 1);
    assert!(result
        .source_fidelity()
        .record_layout()
        .contains_key(curve.id.as_str()));
}
