// SPDX-License-Identifier: Apache-2.0
//! Index allocation admission for a complete one-row DisplayJT stream.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::container::{Container, DirEntry, DirEntryBody, Region};

fn one_row_index() -> Container<'static> {
    let mut data = Vec::new();
    data.extend_from_slice(&9u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&24u32.to_le_bytes());
    data.extend_from_slice(b"Version ");
    Container {
        data: data.into(),
        physical_size: 32,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: Region::Footer,
            body: DirEntryBody::File { offset: 0, len: 32 },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn refused_at(
    mut policy: DecodePolicy,
    collection_limit: Option<u64>,
    retained_limit: Option<u64>,
) -> (ResourceDimension, String) {
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained_limit {
        policy.limits.max_retained_bytes = limit;
    }
    let container = one_row_index();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::display_jt_indices(Some(&ctx), &container).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected resource refusal");
    };
    (limit.dimension, limit.operation.to_string())
}

#[test]
fn display_jt_index_row_count_refuses_before_vector_reservation() {
    assert_eq!(
        refused_at(DecodePolicy::service(), Some(0), None),
        (
            ResourceDimension::CollectionItems,
            "admit DisplayJT index rows".to_string()
        )
    );
}

#[test]
fn display_jt_index_row_storage_refuses_before_vector_reservation() {
    let bytes = std::mem::size_of::<super::DisplayJtIndexRow>() as u64;
    assert_eq!(
        refused_at(DecodePolicy::service(), None, Some(bytes - 1)),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT index rows".to_string()
        )
    );
}

#[test]
fn display_jt_index_row_identity_refuses_before_format_allocation() {
    let rows = std::mem::size_of::<super::DisplayJtIndexRow>() as u64;
    let row_id = "nx:display-jt:index#0-row-0".len() as u64;
    assert_eq!(
        refused_at(DecodePolicy::service(), None, Some(rows + row_id - 1)),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT index row identity".to_string()
        )
    );
}

#[test]
fn display_jt_index_identity_refuses_before_format_allocation() {
    let rows = std::mem::size_of::<super::DisplayJtIndexRow>() as u64;
    let row_id = "nx:display-jt:index#0-row-0".len() as u64;
    let index_id = "nx:display-jt:index#0".len() as u64;
    assert_eq!(
        refused_at(
            DecodePolicy::service(),
            None,
            Some(rows + row_id + index_id - 1)
        ),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT index identity".to_string()
        )
    );
}

#[test]
fn display_jt_index_result_count_refuses_before_vector_reservation() {
    assert_eq!(
        refused_at(DecodePolicy::service(), Some(1), None),
        (
            ResourceDimension::CollectionItems,
            "admit DisplayJT index".to_string()
        )
    );
}

#[test]
fn display_jt_index_result_storage_refuses_before_vector_reservation() {
    let rows = std::mem::size_of::<super::DisplayJtIndexRow>() as u64;
    let row_id = "nx:display-jt:index#0-row-0".len() as u64;
    let index_id = "nx:display-jt:index#0".len() as u64;
    let index = std::mem::size_of::<super::DisplayJtIndex>() as u64;
    assert_eq!(
        refused_at(
            DecodePolicy::service(),
            None,
            Some(rows + row_id + index_id + index - 1)
        ),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT index".to_string()
        )
    );
}

#[test]
fn display_jt_index_service_profile_keeps_one_row() {
    let container = one_row_index();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let indices = super::display_jt_indices(Some(&ctx), &container).unwrap();
    assert_eq!(indices.len(), 1);
    assert_eq!(indices[0].declared_count(), 1);
    assert_eq!(indices[0].rows().next().unwrap().header_offset, 24);
}
