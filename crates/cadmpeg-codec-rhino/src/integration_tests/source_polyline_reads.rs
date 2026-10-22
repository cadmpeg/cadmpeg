// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
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
fn public_polyline_decode_installs_exact_native_reads_under_the_kept_curve() {
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
    let layout = &result.source_fidelity().record_layout()[curve.id.as_str()];
    assert_eq!(
        layout.offset(),
        u64::try_from(range.start).expect("small start")
    );
    assert_eq!(
        layout.byte_len(),
        u64::try_from(range.len()).expect("small extent")
    );
    let fields_end = range.start + payload().len();
    assert_eq!(
        layout.reads()["file"],
        [u64::try_from(range.start).expect("small start")
            ..u64::try_from(fields_end).expect("small fields end")]
    );
    assert!(fields_end < range.end);
}

#[test]
fn detached_polyline_output_keeps_the_same_curve_without_source_captures() {
    let bytes = archive();
    let source = RhinoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("source polyline");
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
        .expect("no source capture invented")
        .is_empty());
}

#[test]
fn a_foreign_registered_polyline_view_cannot_enter_the_producer_scope() {
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
        .expect_err("foreign source cannot enter the polyline record scope");
    assert!(failure
        .to_string()
        .contains("another or detached decode session"));
    assert!(second
        .take_record_captures()
        .expect("no foreign capture committed")
        .is_empty());
}

#[test]
fn a_polyline_with_mismatched_counts_leaves_no_capture_and_its_sibling_survives() {
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
    assert_eq!(result.source_fidelity().record_layout().len(), 1);
    assert!(result
        .source_fidelity()
        .record_layout()
        .contains_key(curve.id.as_str()));
}
