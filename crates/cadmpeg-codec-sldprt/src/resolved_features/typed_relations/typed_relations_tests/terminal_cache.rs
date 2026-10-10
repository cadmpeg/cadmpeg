//! Terminal-profile qualification, cached misses and index lifetime.

use super::super::legacy_terminal_indexed_profile_line_cached;
use crate::records::{SketchInputEntity, SketchInputKind};
use crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::OnceCell;

const INDEX: &str = "index SLDPRT legacy terminal profile lines";
const LOOKUP: &str = "resolve SLDPRT legacy terminal indexed profile line";

fn terminal_fixture() -> (Vec<u8>, SketchInputEntity, SketchInputEntity) {
    let mut payload = vec![0; 168];
    for offset in [0, 84] {
        payload[offset..offset + LEGACY_EXTENDED_SKETCH_MARKER.len()]
            .copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
        payload[offset + 5..offset + 13].fill(0xff);
        payload[offset + 13..offset + 17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[offset + 23..offset + 29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
        payload[offset + 31..offset + 39]
            .copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
        payload[offset + 48..offset + 56].copy_from_slice(&1.0f64.to_le_bytes());
        payload[offset + 60..offset + 64].copy_from_slice(&1u32.to_le_bytes());
        payload[offset + 64..offset + 72].copy_from_slice(&(-1.0f64).to_le_bytes());
    }
    let mut sibling =
        SketchInputEntity::new("sibling", "lane", 0, 0, SketchInputKind::LineOrCircle);
    sibling.feature_ref = Some("sketch".into());
    let mut terminal = SketchInputEntity::new("terminal", "lane", 1, 84, SketchInputKind::Arc);
    terminal.feature_ref = Some("sketch".into());
    (payload, sibling, terminal)
}

#[test]
fn terminal_cache_unqualified_marker_skips_index_and_lookup() {
    let (payload, sibling, terminal) = terminal_fixture();
    let roster = [&sibling, &terminal];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cache = OnceCell::new();
    for _ in 0..64 {
        assert!(!legacy_terminal_indexed_profile_line_cached(
            &ctx, &payload, &sibling, &roster, &cache
        )
        .unwrap());
    }
    assert!(cache.get().is_none());
    assert_eq!(ctx.resource_refusal(), None);
    policy.limits.max_work_units = u64::MAX;
    for operation in [INDEX, LOOKUP] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let cache = OnceCell::new();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        assert!(!legacy_terminal_indexed_profile_line_cached(
            &ctx, &payload, &sibling, &roster, &cache
        )
        .unwrap());
        assert!(cache.get().is_none());
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, operation) else {
            panic!("unqualified terminal input must leave its probe armed");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.charge_work(0, "after unused terminal operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        drop(cache);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        drop(probe);
    }
}

#[test]
fn terminal_cache_reuses_positive_and_negative_indexes() {
    assert_eq!(std::mem::size_of::<(Option<&str>, u64)>(), 24);
    let (payload, sibling, terminal) = terminal_fixture();
    for positive in [false, true] {
        let roster = if positive {
            vec![&sibling, &terminal]
        } else {
            vec![&terminal]
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        // The positive index has four 24-byte buckets plus alignment and control bytes.
        let materialized = if positive { 4 * 24 + 15 + 4 + 16 } else { 0 };
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_collection_items = u64::from(positive);
        policy.limits.max_work_units = u64::MAX;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let cache = OnceCell::new();
        assert_eq!(
            legacy_terminal_indexed_profile_line_cached(&ctx, &payload, &terminal, &roster, &cache)
                .unwrap(),
            positive
        );
        assert!(cache.get().is_some());
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            INDEX,
            None,
        );
        for _ in 0..64 {
            assert_eq!(
                legacy_terminal_indexed_profile_line_cached(
                    &ctx, &payload, &terminal, &roster, &cache
                )
                .unwrap(),
                positive
            );
        }
        assert_eq!(ctx.resource_refusal(), None);
        drop(cache);
        let released = ctx
            .reserve_scoped(materialized, "released terminal index backing")
            .unwrap();
        drop(released);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, INDEX) else {
            panic!("cached hit and miss must leave the index probe armed");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, INDEX);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.charge_work(0, "after cached terminal index"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        drop(probe);
    }
}

#[test]
fn terminal_cache_refusal_leaves_index_unpublished() {
    let (payload, sibling, terminal) = terminal_fixture();
    let roster = [&sibling, &terminal];
    for (dimension, cap, used, additional) in [
        // admit_iter admits the complete two-marker roster before its first visit.
        (ResourceDimension::WorkUnits, 0, 0, 2),
        (ResourceDimension::MaterializedBytes, 130, 0, 131),
        (ResourceDimension::CollectionItems, 0, 0, 1),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 131;
        policy.limits.max_collection_items = 1;
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            _ => panic!("terminal index resource dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let cache = OnceCell::new();
        let Err(CodecError::ResourceLimit(limit)) =
            legacy_terminal_indexed_profile_line_cached(&ctx, &payload, &terminal, &roster, &cache)
        else {
            panic!("terminal index must admit its source and backing");
        };
        assert!(cache.get().is_none());
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, INDEX);
        assert_eq!(limit.limit, cap);
        assert_eq!(limit.used, used);
        assert_eq!(limit.additional, additional);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(
            matches!(ctx.charge_work(0, "after terminal index refusal"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
        drop(cache);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}
