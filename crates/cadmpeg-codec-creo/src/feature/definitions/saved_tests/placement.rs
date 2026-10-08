// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn decodes_zero_offset_positional_placement_instruction() {
    let payload = b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
            \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
    let rows = crate::decode::with_test_decode_ctx(|ctx| -> Result<_, CodecError> {
        placement_instruction_rows(ctx, payload, 1000)?.collect(ctx)
    })
    .expect("placement instruction search is admitted");
    let [row] = rows.as_slice() else {
        panic!("placement row");
    };
    assert_eq!(row.kind, 20_127);
    assert!(row.zero_offset);
    assert_eq!(row.dimension_id, None);
    assert_eq!(row.reference_id, None);
    assert_eq!(row.geometry1_id, Some(2));
    assert_eq!(row.geometry2_id, None);
    assert_eq!([row.member1, row.member2], [0, 0]);
    assert_eq!(row.offset, 1029);
}

#[test]
fn placement_instruction_projection_refuses_before_byte_traversal() {
    let payload = b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
            \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
    let rows = crate::test_support::assert_work_boundaries(
        &["creo placement instruction byte traversal"],
        |ctx| placement_instruction_rows(ctx, payload, 1000)?.collect(ctx),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, 20_127);
    assert_eq!(rows[0].offset, 1029);
    assert_eq!(rows[0].geometry1_id, Some(2));
}
