// SPDX-License-Identifier: Apache-2.0
//! Index allocation admission for a complete one-row `DisplayJT` stream.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
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
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn refused_at(
    adjust: impl FnOnce(&mut DecodePolicy),
    collection_limit: Option<u64>,
    materialized_limit: Option<u64>,
) -> (ResourceDimension, String) {
    let container = one_row_index();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            adjust(policy);
            if let Some(limit) = collection_limit {
                policy.limits.max_collection_items = limit;
            }
            if let Some(limit) = materialized_limit {
                policy.limits.max_materialized_bytes = limit;
            }
        },
        |ctx| {
            let error = super::display_jt_indices(ctx, &container).unwrap_err();
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected resource refusal");
            };
            assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
            (limit.dimension, limit.operation.to_string())
        },
    )
}

#[test]
fn display_jt_index_row_count_refuses_before_vector_reservation() {
    assert_eq!(
        refused_at(|_| {}, Some(0), None),
        (
            ResourceDimension::CollectionItems,
            "admit DisplayJT index rows".to_string()
        )
    );
}

#[test]
fn display_jt_index_row_storage_refuses_before_vector_reservation() {
    let bytes =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<super::DisplayJtIndexRow>());
    assert_eq!(
        refused_at(|_| {}, None, Some(bytes - 1)),
        (
            ResourceDimension::MaterializedBytes,
            "retain DisplayJT index rows".to_string()
        )
    );
}

#[test]
fn display_jt_index_row_entity_refuses_before_identity_allocation() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_entities = 0;
    };
    assert_eq!(
        refused_at(adjust_policy, None, None),
        (
            ResourceDimension::Entities,
            "admit DisplayJT index row".to_string()
        )
    );
}

#[test]
fn display_jt_index_row_identity_refuses_before_format_allocation() {
    let rows =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<super::DisplayJtIndexRow>());
    let row_id = cadmpeg_core::decode::u64_from_index("nx:display-jt:index#0-row-0".len());
    assert_eq!(
        refused_at(|_| {}, None, Some(rows + row_id - 1)),
        (
            ResourceDimension::MaterializedBytes,
            "retain DisplayJT index row identity".to_string()
        )
    );
}

#[test]
fn display_jt_index_identity_refuses_before_format_allocation() {
    let rows =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<super::DisplayJtIndexRow>());
    let row_id = cadmpeg_core::decode::u64_from_index("nx:display-jt:index#0-row-0".len());
    let index_id = cadmpeg_core::decode::u64_from_index("nx:display-jt:index#0".len());
    assert_eq!(
        refused_at(|_| {}, None, Some(rows + row_id + index_id - 1)),
        (
            ResourceDimension::MaterializedBytes,
            "retain DisplayJT index identity".to_string()
        )
    );
}

#[test]
fn display_jt_index_result_count_refuses_before_vector_reservation() {
    assert_eq!(
        refused_at(|_| {}, Some(1), None),
        (
            ResourceDimension::CollectionItems,
            "admit DisplayJT index".to_string()
        )
    );
}

#[test]
fn display_jt_index_result_storage_refuses_before_vector_reservation() {
    let index =
        cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<super::DisplayJtIndex>());
    assert_eq!(
        refused_at(
            |policy| policy.limits.max_retained_bytes = index - 1,
            None,
            None,
        ),
        (
            ResourceDimension::RetainedBytes,
            "admit DisplayJT index".to_string()
        )
    );
}


#[test]
fn display_jt_index_candidate_commits_exact_retained_storage() {
    // Four row slots, four outer index slots, and the two exact identities.
    let bytes = cadmpeg_core::decode::u64_from_index(
        4 * std::mem::size_of::<super::DisplayJtIndexRow>()
            + 4 * std::mem::size_of::<super::DisplayJtIndex>()
            + "nx:display-jt:index#0-row-0".len() + "nx:display-jt:index#0".len(),
    );
    let container = one_row_index();
    for cap in [bytes - 1, bytes] {
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_retained_bytes = cap;
        }, |ctx| {
            let result = super::display_jt_indices(ctx, &container);
            if cap == bytes {
                let indices = result.expect("the exact accepted output fits");
                assert_eq!(indices.len(), 1);
                assert_eq!(indices[0].rows().next().unwrap().header_offset, 24);
                assert!(ctx.resource_refusal().is_none());
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("one byte below final storage must refuse");
                };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "DisplayJT index candidate");
                assert_eq!(limit.used + limit.additional, bytes);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        });
    }
}

#[test]
fn display_jt_index_service_profile_keeps_one_row() {
    let container = one_row_index();

    crate::test_support::with_decode_context(|ctx| {
        let indices = super::display_jt_indices(ctx, &container).unwrap();
        assert_eq!(indices.len(), 1);
        assert_eq!(indices[0].declared_count(), 1);
        assert_eq!(indices[0].rows().next().unwrap().header_offset, 24);
    });
}
