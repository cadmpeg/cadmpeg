// SPDX-License-Identifier: Apache-2.0

use super::{stream_prefix, version_context_stream};
use super::super::{parse_inner, take_record_index, take_version_context, ParseIssue};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn rejected_work(bytes: &[u8], field: &str, offset: usize) -> u64 {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(matches!(parse_inner(ctx, bytes),
            Err(ParseIssue::Malformed(failure)) if failure.field == field && failure.offset == offset));
        assert!(ctx.resource_refusal().is_none());
        let Err(CodecError::ResourceLimit(limit)) =
            ctx.charge_work(u64::MAX, "measure reached MetaStream rows")
        else { panic!("actual reached row work measurement"); };
        limit.used
    })
}

#[test]
fn metastream_reached_rows_invalid_first_type_does_not_visit_declared_tail() {
    let mut short = stream_prefix();
    let count_at = short.len() - 16;
    short[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let mut long = short.clone();
    long[count_at..count_at + 4].copy_from_slice(&64u32.to_le_bytes());
    assert_eq!(rejected_work(&short, "type GUID", count_at + 4),
        rejected_work(&long, "type GUID", count_at + 4));
}

#[test]
fn metastream_reached_rows_invalid_first_property_does_not_visit_declared_tail() {
    let mut short = version_context_stream(&[]);
    let count_at = short.len() - 4;
    short[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let mut long = short.clone();
    long[count_at..count_at + 4].copy_from_slice(&4096u32.to_le_bytes());
    assert_eq!(rejected_work(&short, "property name", count_at + 4),
        rejected_work(&long, "property name", count_at + 4));
}

#[test]
fn metastream_reached_rows_invalid_first_context_does_not_visit_declared_tail() {
    let actual_work = |count: u32| {
        let bytes = count.to_le_bytes();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut at = 0;
            assert!(matches!(take_version_context(ctx, &bytes, &mut at),
                Err(ParseIssue::Malformed(failure)) if failure.field == "version-context token" && failure.offset == 4));
            assert!(ctx.resource_refusal().is_none());
            let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(
                u64::MAX, "measure reached version context",
            ) else { panic!("actual version source measurement"); };
            limit.used
        })
    };
    assert_eq!(actual_work(1), 1);
    assert_eq!(actual_work(64), 1);
}

#[test]
fn metastream_reached_rows_record_source_refuses_one_actual_attempt() {
    for count in [1u32, 4096] {
        let mut bytes = count.to_le_bytes().to_vec();
        bytes.resize(4 + usize::try_from(count).unwrap() * 16, 0);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut at = 0;
            let Err(CodecError::ResourceLimit(original)) = take_record_index(ctx, &bytes, &mut at)
            else { panic!("first actual record visit must refuse"); };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.used, 0);
            assert_eq!(original.additional, 1);
            assert_eq!(original.operation, "visit F3D MetaStream record index");
            assert_eq!(at, 0);
            let Err(CodecError::ResourceLimit(later)) = take_record_index(ctx, &[0; 4], &mut at)
            else { panic!("known-empty record source must keep original refusal"); };
            assert_eq!(later, original);
            assert_eq!(ctx.resource_refusal(), Some(original));
        });
    }
}

#[test]
fn metastream_reached_rows_empty_sources_are_free_and_keep_original_fuse() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut at = 0;
        assert!(take_record_index(ctx, &[0; 4], &mut at).unwrap().unwrap().is_empty());
        assert_eq!(at, 4);
        at = 0;
        assert!(take_version_context(ctx, &[0; 4], &mut at).is_ok());
        assert_eq!(at, 4);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "empty MetaStream row source refusal")
        else { panic!("zero-work original session refusal"); };
        at = 0;
        assert!(matches!(take_version_context(ctx, &[0; 4], &mut at),
            Err(ParseIssue::Resource(CodecError::ResourceLimit(later))) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
