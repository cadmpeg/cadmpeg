// SPDX-License-Identifier: Apache-2.0
//! Document and table-of-contents allocation admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::container::{Container, DirEntry, DirEntryBody, Region};

fn one_document() -> Container<'static> {
    let mut data = Vec::new();
    data.extend_from_slice(&9u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&100u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&28u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&[0; 4]);
    data.extend_from_slice(&105u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137u32.to_le_bytes());
    data.extend_from_slice(&24u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&[0; 24]);
    let data_len = data.len() as u64;
    Container {
        data: data.into(),
        physical_size: data_len,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: Region::Footer,
            body: DirEntryBody::File {
                offset: 0,
                len: data_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn refused_at(policy: DecodePolicy) -> (ResourceDimension, String) {
    let container = one_document();
    let indices = super::display_jt_indices(None, &container).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::display_jt_documents(Some(&ctx), &container, &indices).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected resource refusal");
    };
    (limit.dimension, limit.operation.to_string())
}

#[test]
fn display_jt_version_refuses_before_string_copy() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 79;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT version text".to_string()
        )
    );
}

#[test]
fn display_jt_toc_count_refuses_before_vector_reservation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::CollectionItems,
            "admit DisplayJT toc entries".to_string()
        )
    );
}

#[test]
fn display_jt_toc_storage_refuses_before_vector_reservation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        80 + std::mem::size_of::<super::DisplayJtTocEntry>() as u64 - 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT toc entries".to_string()
        )
    );
}

#[test]
fn display_jt_toc_entity_refuses_before_identity_allocation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::Entities,
            "admit DisplayJT toc entry".to_string()
        )
    );
}

#[test]
fn display_jt_toc_identity_refuses_before_format_allocation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 80
        + std::mem::size_of::<super::DisplayJtTocEntry>() as u64
        + "nx:display-jt:toc-entry#0-0".len() as u64
        - 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT toc identity".to_string()
        )
    );
}

#[test]
fn display_jt_document_entity_refuses_before_identity_allocation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::Entities,
            "admit DisplayJT document entity".to_string()
        )
    );
}

#[test]
fn display_jt_document_identity_refuses_before_format_allocation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 80
        + std::mem::size_of::<super::DisplayJtTocEntry>() as u64
        + "nx:display-jt:toc-entry#0-0".len() as u64
        + "nx:display-jt:document#0".len() as u64
        - 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT document identity".to_string()
        )
    );
}

#[test]
fn display_jt_document_index_reference_refuses_before_clone() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 80
        + std::mem::size_of::<super::DisplayJtTocEntry>() as u64
        + "nx:display-jt:toc-entry#0-0".len() as u64
        + "nx:display-jt:document#0".len() as u64
        + "nx:display-jt:index#0-row-0".len() as u64
        - 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT document index reference".to_string()
        )
    );
}

#[test]
fn display_jt_document_storage_refuses_before_vector_reservation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 80
        + std::mem::size_of::<super::DisplayJtTocEntry>() as u64
        + "nx:display-jt:toc-entry#0-0".len() as u64
        + "nx:display-jt:document#0".len() as u64
        + "nx:display-jt:index#0-row-0".len() as u64
        + std::mem::size_of::<super::DisplayJtDocument>() as u64
        - 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::RetainedBytes,
            "retain DisplayJT document".to_string()
        )
    );
}

#[test]
fn display_jt_document_work_refuses_before_toc_scan() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::WorkUnits,
            "scan DisplayJT table of contents".to_string()
        )
    );
}

#[test]
fn display_jt_document_count_refuses_before_vector_reservation() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    assert_eq!(
        refused_at(policy),
        (
            ResourceDimension::CollectionItems,
            "admit DisplayJT document".to_string()
        )
    );
}

#[test]
fn display_jt_document_service_profile_keeps_toc_entry() {
    let container = one_document();
    let indices = super::display_jt_indices(None, &container).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let documents = super::display_jt_documents(Some(&ctx), &container, &indices).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].toc_entries.len(), 1);
    assert_eq!(documents[0].toc_entries[0].segment_offset, 137);
}
