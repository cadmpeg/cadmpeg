// SPDX-License-Identifier: Apache-2.0
//! Borrowed XREF text and output-storage admission controls.

use super::{legacy_occurrence_record, occurrence_record, repeated_target_occurrence_record};
use crate::test_support::cross_document_reference;
use crate::xref::{
    legacy_occurrence_prefix, occurrence_path, repeated_target_occurrence_placement_details,
};

#[test]
fn occurrence_path_keeps_one_link_copy_and_no_header_or_inline_guid_copy() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    const LINK: &str = "A😀";
    const GUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let mut bytes = occurrence_record(LINK, 7, &[1], None);
    let name = "n".repeat(256);
    let header_name = 256_u32.to_le_bytes().into_iter().chain(name.bytes());
    bytes.splice(15..19, header_name);
    let reference_at = 24 + name.len();
    let inline_guid = 36_u32.to_le_bytes().into_iter().chain(GUID.bytes()).collect::<Vec<_>>();
    bytes.splice(reference_at + 9..reference_at + 9, inline_guid.iter().copied());
    let required = u64::try_from(4 * std::mem::size_of::<String>() + LINK.len()).unwrap();
    let end = reference_at + cross_document_reference(100, LINK).len() + inline_guid.len() + 5;
    for cap in [required, required - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 1;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = occurrence_path(ctx, &bytes);
            if cap == required {
                let (links, at) = result.unwrap().unwrap();
                assert_eq!(links, [LINK]);
                assert_eq!(at, end);
                assert_eq!(links.capacity(), 4);
                assert_eq!(links[0].capacity(), LINK.len());
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let cadmpeg_core::CodecError::ResourceLimit(limit) = result.unwrap_err()
                else { panic!("the output backing must refuse one byte below its actual size"); };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "collect F3D xref placement link names");
                assert_eq!(limit.used, u64::try_from(LINK.len()).unwrap());
                assert_eq!(limit.additional, u64::try_from(4 * std::mem::size_of::<String>()).unwrap());
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        });
    }
}

#[test]
fn occurrence_path_with_only_local_targets_needs_no_decode_storage() {
    use cadmpeg_core::decode::DecodePolicy;
    const GUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    bytes.extend_from_slice(&256_u32.to_le_bytes());
    bytes.extend(std::iter::repeat_n(b'n', 256));
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&100_u64.to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(GUID.as_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let (links, at) = occurrence_path(ctx, &bytes).unwrap().unwrap();
        assert!(links.is_empty());
        assert_eq!(links.capacity(), 0);
        assert_eq!(at, bytes.len());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn repeated_target_placement_retains_only_its_published_role_and_path() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    const ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    const GUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let mut bytes = repeated_target_occurrence_record(ROLE, 10, 1, None);
    let tail_guid = 36_u32.to_le_bytes().into_iter().chain(GUID.bytes());
    // The final local reference's type GUID is grammar-only.
    let at = bytes.len() - 2;
    bytes.splice(at..at, tail_guid);
    // The four-slot path vector, its one link, and the published role.
    let required = u64::try_from(4 * std::mem::size_of::<String>() + 2 * ROLE.len()).unwrap();
    for cap in [required, required - 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        policy.limits.max_collection_items = 1;
        policy.limits.max_materialized_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = repeated_target_occurrence_placement_details(ctx, &bytes);
            if cap == required {
                let details = result.unwrap().unwrap();
                assert_eq!(details.link_names, [ROLE]);
                assert_eq!(details.role, ROLE);
                assert_eq!(details.transform, None);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let error = match result { Err(error) => error, Ok(_) => panic!("the published role must refuse one byte below its storage") };
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error
                else { panic!("published role storage must retain the original refusal"); };
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.operation, "retain F3D UTF-16 string");
                assert_eq!(limit.used, u64::try_from(4 * std::mem::size_of::<String>() + ROLE.len()).unwrap());
                assert_eq!(limit.additional, u64::try_from(ROLE.len()).unwrap());
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        });
    }
    // A suffix prevents role publication; no owned role copy is needed.
    bytes.push(0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(4 * std::mem::size_of::<String>() + ROLE.len()).unwrap();
    policy.limits.max_collection_items = 1;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(repeated_target_occurrence_placement_details(ctx, &bytes).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn legacy_occurrence_name_validation_uses_the_original_context() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    let mut bytes = legacy_occurrence_record("aaaabbbb-cccc-dddd-eeee-ffff00001111", 10, None);
    let name = 256_u32.to_le_bytes().into_iter().chain(std::iter::repeat_n(0xff, 256));
    bytes.splice(15..19, name);
    let mut policy = DecodePolicy::service();
    // The class tag is admitted first. The name must refuse before validation.
    policy.limits.max_work_units = 3;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            legacy_occurrence_prefix(ctx, &bytes).unwrap_err()
        else { panic!("the name validation must refuse before its UTF-8 error"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "decode F3D ASCII string");
        assert_eq!(limit.used, 3);
        assert_eq!(limit.additional, 256);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    policy.limits.max_work_units = 3 + 256;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(legacy_occurrence_prefix(ctx, &bytes).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}
