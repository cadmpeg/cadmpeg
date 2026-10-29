// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use crate::test_support::test_archive;
use crate::RhinoCodec;
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
fn public_point_decode_keeps_the_point_before_its_unparsed_extension() {
    let bytes = archive();
    let scan = crate::container::scan_owned(bytes.clone()).expect("bounded source");
    let range = scan.objects[0]
        .framed()
        .expect("point record")
        .class_data_range
        .clone();
    let result = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("source point");
    let point = &result.ir().model.points[0];
    assert_eq!(
        point.position().get(),
        cadmpeg_ir::math::Point3::new(1.25, -2.5, 3.75)
    );
    let fields_end = range.start + test_archive::point_payload([1.25, -2.5, 3.75]).len();
    assert!(fields_end < range.end);
}

#[test]
fn an_unformed_point_keeps_its_valid_sibling() {
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
}
