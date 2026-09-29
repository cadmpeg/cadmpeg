// SPDX-License-Identifier: Apache-2.0
//! Scene-node parser and record allocation admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn display_jt_base_node_attributes_refuse_before_counted_vector() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::admit_jt_base_body(&ctx, &body, 9, true).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "decode DisplayJT base node attributes"));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(super::admit_jt_base_body(&service, &body, 9, true)
        .unwrap()
        .is_some());
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::admit_jt_group_body(&ctx, &body).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "decode DisplayJT group children"));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(super::admit_jt_group_body(&service, &body)
        .unwrap()
        .is_some());
}

#[test]
fn display_jt_partition_name_refuses_before_utf16_allocation() {
    let mut family = Vec::new();
    family.extend_from_slice(&0_u32.to_le_bytes());
    family.extend_from_slice(&1_u32.to_le_bytes());
    family.extend_from_slice(&u16::from(b'x').to_le_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::admit_jt_partition_name(&ctx, &family).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "decode DisplayJT partition name"));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(matches!(
        super::admit_jt_partition_name(&service, &family).unwrap(),
        super::JtOptionalReservation::Admitted(_)
    ));
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::admit_jt_range_vectors(&ctx, &family).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "decode DisplayJT range values"));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(matches!(
        super::admit_jt_range_vectors(&service, &family).unwrap(),
        super::JtOptionalReservation::Admitted(_)
    ));
}
