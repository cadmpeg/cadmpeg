// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

fn archive() -> Vec<u8> {
    let mut payload = test_archive::point_payload([1.25, -2.5, 3.75]);
    payload.extend(b"unparsed-extension");
    test_archive::archive(&[test_archive::object_record(
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &payload,
    )])
}

#[test]
fn public_point_decode_installs_actual_scalar_reads_under_the_kept_point() {
    let bytes = archive();
    let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
    let range = scan.objects[0]
        .framed()
        .expect("point record")
        .class_data_range();
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source point");
    let point = &result.ir().model.points[0];
    assert_eq!(
        point.position().get(),
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    let layout = &result.source_fidelity().record_layout()[point.id.as_str()];
    assert_eq!(
        layout.offset(),
        u64::try_from(range.start).expect("small point start")
    );
    assert_eq!(
        layout.byte_len(),
        u64::try_from(range.len()).expect("small point extent")
    );
    let fields_end = range.start + test_archive::point_payload([1.25, -2.5, 3.75]).len();
    assert_eq!(
        layout.reads()["file"],
        [u64::try_from(range.start).expect("small point start")
            ..u64::try_from(fields_end).expect("small point end")]
    );
    assert!(fields_end < range.end);
}

#[test]
fn detached_authored_output_uses_the_same_fields_without_fabricating_source_captures() {
    let bytes = archive();
    let source = RhinoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("source point");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let detached = RhinoCodec
        .decode_with_context(&ctx, View::over_retained(&bytes))
        .expect("pure authored-output parse");
    assert_eq!(detached.ir().model.points, source.ir().model.points);
    assert!(detached.source_fidelity().record_layout().is_empty());
    assert!(ctx
        .take_record_captures()
        .expect("no source capture invented")
        .is_empty());
}

#[test]
fn a_registered_view_from_another_session_is_not_treated_as_detached_output() {
    let bytes = archive();
    let first_arena = DecodeArena::new();
    let (_first, source) =
        DecodeContext::from_root_bytes(&bytes, &first_arena, &DecodePolicy::default())
            .expect("first session");
    let second_arena = DecodeArena::new();
    let (second, _) =
        DecodeContext::from_root_bytes(b"other", &second_arena, &DecodePolicy::default())
            .expect("second session");
    assert!(source.has_source_tracking());
    let failure = RhinoCodec
        .decode_with_context(&second, source)
        .expect_err("foreign source cannot enter the record scope");
    assert!(failure
        .to_string()
        .contains("another or detached decode session"));
    assert!(second
        .take_record_captures()
        .expect("no foreign capture committed")
        .is_empty());
}

#[test]
fn an_unformed_point_keeps_no_read_capture_and_its_valid_sibling_survives() {
    let bytes = test_archive::archive(&[
        test_archive::object_record(
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &test_archive::point_payload([f64::NAN, 0.0, 0.0]),
        ),
        test_archive::object_record(
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &test_archive::point_payload([1.25, -2.5, 3.75]),
        ),
    ]);
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("bounded malformed point does not remove its valid sibling");
    assert_eq!(result.ir().model.points.len(), 1);
    let point = &result.ir().model.points[0];
    assert_eq!(
        point.position().get(),
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    assert_eq!(result.source_fidelity().record_layout().len(), 1);
    assert!(result
        .source_fidelity()
        .record_layout()
        .contains_key(point.id.as_str()));
}
