// SPDX-License-Identifier: Apache-2.0
//! Scene-node parsers.

#[test]
fn display_jt_base_node_attributes_read_counted_object_ids() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());

    assert_eq!(
        super::read_jt_object_ids(
            super::parse_jt_base_node_body(&body, 9)
                .expect("bounded attributes")
                .2
        ),
        [7]
    );
}

#[test]
fn display_jt_group_children_read_counted_object_ids() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());

    assert_eq!(
        super::read_jt_object_ids(
            super::parse_jt9_group_node_body(&body)
                .expect("bounded children")
                .1
        ),
        [9]
    );
}

#[test]
fn display_jt_partition_name_decodes_utf16() {
    let mut family = Vec::new();
    family.extend_from_slice(&0_u32.to_le_bytes());
    family.extend_from_slice(&1_u32.to_le_bytes());
    family.extend_from_slice(&u16::from(b'x').to_le_bytes());
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&family);
    for _ in 0..13 {
        body.extend_from_slice(&0_f32.to_le_bytes());
    }
    for _ in 0..6 {
        body.extend_from_slice(&0_i32.to_le_bytes());
    }
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::parse_jt9_partition_node_body(ctx, &body)
                .expect("partition name decodes")
                .expect("complete partition")
                .file_name,
            "x"
        );
    });
}

#[test]
fn display_jt_range_vectors_read_each_counted_vector() {
    let mut family = Vec::new();
    family.extend_from_slice(&1_u16.to_le_bytes());
    family.extend_from_slice(&1_u32.to_le_bytes());
    family.extend_from_slice(&0.5_f32.to_le_bytes());
    family.extend_from_slice(&0_i32.to_le_bytes());
    family.extend_from_slice(&1_u16.to_le_bytes());
    family.extend_from_slice(&1_u32.to_le_bytes());
    family.extend_from_slice(&1.0_f32.to_le_bytes());

    let mut view = super::View::over_retained(&family[2..]);
    let first = super::parse_jt_f32_vector(&mut view).expect("first vector");
    view.take(6).expect("reserved value and range version");
    let second = super::parse_jt_f32_vector(&mut view).expect("second vector");
    assert_eq!([first[0].get(), second[0].get()], [0.5, 1.0]);
}
