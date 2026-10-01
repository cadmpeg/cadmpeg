// SPDX-License-Identifier: Apache-2.0
//! Scene-node parser and record allocation admission.

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn display_jt_base_node_attributes_refuse_before_counted_vector() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = super::read_jt_object_ids(ctx, super::parse_jt_base_node_body(&body, 9).expect("bounded attributes").2, "decode DisplayJT base node attributes").unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "decode DisplayJT base node attributes"));
            crate::test_support::with_decode_context(|service| {
                assert!(super::read_jt_object_ids(service, super::parse_jt_base_node_body(&body, 9).expect("bounded attributes").2, "decode DisplayJT base node attributes").is_ok());
            });
        },
    );
}

#[test]
fn display_jt_group_children_refuse_before_counted_vector() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = super::read_jt_object_ids(ctx, super::parse_jt9_group_node_body(&body).expect("bounded children").1, "decode DisplayJT group children").unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "decode DisplayJT group children"));
            crate::test_support::with_decode_context(|service| {
                assert!(super::read_jt_object_ids(service, super::parse_jt9_group_node_body(&body).expect("bounded children").1, "decode DisplayJT group children").is_ok());
            });
        },
    );
}

#[test]
fn display_jt_partition_name_refuses_before_utf16_allocation() {
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
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error = super::parse_jt9_partition_node_body(ctx, &body)
                .err()
                .expect("retained refusal");
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 1
                    && limit.operation == "retain DisplayJT partition name"));
            crate::test_support::with_decode_context_over(
                &[],
                |policy| {
                    policy.limits.max_retained_bytes = 1;
                    policy.limits.max_materialized_bytes = 0;
                },
                |service| {
                    assert_eq!(
                        super::parse_jt9_partition_node_body(service, &body)
                            .expect("exact retained budget")
                            .expect("complete partition")
                            .file_name,
                        "x"
                    );
                },
            );
        },
    );
}

#[test]
fn display_jt_range_vectors_refuse_before_conversion_allocation() {
    let mut family = Vec::new();
    family.extend_from_slice(&1_u16.to_le_bytes());
    family.extend_from_slice(&1_u32.to_le_bytes());
    family.extend_from_slice(&0.5_f32.to_le_bytes());
    family.extend_from_slice(&0_i32.to_le_bytes());
    family.extend_from_slice(&1_u16.to_le_bytes());
    family.extend_from_slice(&1_u32.to_le_bytes());
    family.extend_from_slice(&1.0_f32.to_le_bytes());

    let read_vectors = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| -> Result<[Vec<super::FiniteBinary32>; 2], CodecError> {
        let (first, rest) = super::parse_jt_f32_vector(ctx, &family[2..])?.expect("first vector");
        let (second, _) = super::parse_jt_f32_vector(ctx, &rest[6..])?.expect("second vector");
        Ok([first, second])
    };
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_collection_items = 3,
        |ctx| {
            let vectors = read_vectors(ctx).expect("one allocation for each vector fits three slots");
            assert_eq!(vectors.map(|values| values[0].get()), [0.5, 1.0]);
        },
    );
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_collection_items = 1,
        |ctx| {
            let error = read_vectors(ctx).expect_err("second vector requires a second slot");
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "decode DisplayJT range values"));
        },
    );
}
