// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{independent_persistent_references, PersistentReferenceKind};

#[test]
fn one_owned_field_consumes_only_one_same_stream_observation() {
    let bytes = include_bytes!("../../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = crate::F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the unchanged sketch fixture");
    let mut native = crate::test_support::native_test::f3d_native(decoded.ir());
    let point = native.sketch_points[0].clone();
    let value = point
        .persistent_id()
        .expect("the test point has a persistent field");
    let reference = native
        .persistent_references
        .iter()
        .find(|reference| {
            reference.kind == PersistentReferenceKind::Point
                && reference.value == value
                && crate::ids::native_stream(&reference.id) == crate::ids::native_stream(&point.id)
        })
        .expect("the owned point field also occurs in the observation arena")
        .clone();
    native.sketch_points = vec![point];
    native.sketch_curve_identities.clear();
    let mut duplicate = reference.clone();
    duplicate.id.push_str("-independent");
    native.persistent_references = vec![reference, duplicate.clone()];
    let independent = independent_persistent_references(&native);
    assert_eq!(independent.len(), 1);
    assert_eq!(independent[0].id, duplicate.id);
    assert_eq!(independent[0].value, value);
    native.sketch_points.clear();
    assert_eq!(independent_persistent_references(&native).len(), 2);
    native.sketch_points = crate::test_support::native_test::f3d_native(decoded.ir()).sketch_points;
    duplicate.id = "f3d:other-stream:persistent-reference#independent".into();
    native.persistent_references = vec![duplicate];
    assert_eq!(independent_persistent_references(&native).len(), 1);
}
