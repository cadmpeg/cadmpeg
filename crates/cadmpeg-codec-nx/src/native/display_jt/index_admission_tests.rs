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
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn refused_at(adjust: impl FnOnce(&mut DecodePolicy)) -> (ResourceDimension, String) {
    let container = one_row_index();

    crate::test_support::with_decode_context_over(&[], adjust, |ctx| {
        let error = super::display_jt_indices(ctx, &container).unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected resource refusal");
        };
        (limit.dimension, limit.operation.to_string())
    })
}

#[test]
fn display_jt_index_row_entity_refuses_before_identity_allocation() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| {
        policy.limits.max_entities = 0;
    };
    assert_eq!(
        refused_at(adjust_policy),
        (
            ResourceDimension::Entities,
            "admit DisplayJT index row".to_string()
        )
    );
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
