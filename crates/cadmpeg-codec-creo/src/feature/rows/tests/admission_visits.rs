// SPDX-License-Identifier: Apache-2.0

use super::super::{AffectedIdKind, FeatureFieldValue, FeatureGeometryTableKind, FeatureRow};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check_visits(
    total: u64,
    operation_at: impl Fn(u64) -> &'static str,
    parse: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    for cap in 0..=total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = parse(&ctx);
        if cap == total {
            result.expect("exact present visits");
            assert_eq!(ctx.resource_refusal(), None);
            let refusal = ctx.charge_work_limit(1, "after feature row visits").expect_err("exact cap");
            assert_eq!((refusal.dimension, refusal.used, refusal.additional),
                (ResourceDimension::WorkUnits, total, 1));
        } else {
            let original = ctx.resource_refusal().expect("present visit refuses");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, 1, operation_at(cap)));
        }
        let original = ctx.resource_refusal().expect("original refusal");
        assert!(matches!(parse(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn compact_field_visits_match_only_declared_entries() {
    for count in 0..=3u8 {
        let mut payload = vec![0xf8, count];
        payload.extend(1..=count);
        // Three u32 entries fit the initial four-slot capacity: no move work.
        check_visits(u64::from(count), |_| "creo compact integer field traversal", |ctx| {
            assert_eq!(super::super::field_value(ctx, &payload)?,
                FeatureFieldValue::CompactIntArray((1..=u32::from(count)).collect()));
            Ok(())
        });
    }
}

#[test]
fn datum_visits_match_only_present_named_and_positional_entries() {
    for count in 0..=3u8 {
        let mut positional = vec![0xf8, count, 0xf7, 87, 0xe2];
        let mut named = positional.clone();
        for index in 0..count {
            if index == 0 {
                positional.extend_from_slice(&[0xf7, 88]);
            } else {
                positional.extend_from_slice(&[0xf1, 0xf7, 87, 0xe2]);
            }
            positional.extend_from_slice(&[index + 1, 0xf6]);
            named.extend_from_slice(b"\xe0\x01dtm_id\0");
            named.push(index + 1);
        }
        let expected: Vec<u32> = (1..=u32::from(count)).collect();
        check_visits(u64::from(count), |_| "creo positional datum traversal", |ctx| {
            assert_eq!(super::super::positional_datum_geometry_table_at(ctx, &positional, 0, 87)?,
                Some((u32::from(count), expected.clone())));
            Ok(())
        });
        check_visits(u64::from(count), |_| "creo named datum traversal", |ctx| {
            assert_eq!(super::super::geometry_table_at(ctx, &named, 0, FeatureGeometryTableKind::DatumIds(None))?,
                Some((u32::from(count), 87, FeatureGeometryTableKind::DatumIds(Some(expected.clone())))));
            Ok(())
        });
    }
}

#[test]
fn borrowed_replay_visits_match_present_ids_and_preserve_fixed_extent_routes() {
    for count in 0..=3u8 {
        let ids: Vec<u8> = (1..=count).collect();
        check_visits(u64::from(count), |_| "creo replay ID traversal", |ctx| {
            let (decoded, after) = super::super::replay_ids(ctx, &ids, u32::from(count), 0)
                .transpose()?.expect("complete borrowed ids");
            assert_eq!((decoded.bytes, decoded.count, after), (ids.as_slice(), u32::from(count), ids.len()));
            Ok(())
        });
        let mut explicit = vec![0xf8, count];
        explicit.extend_from_slice(&ids);
        check_visits(u64::from(count), |_| "creo replay ID traversal", |ctx| {
            let (decoded, after) = super::super::explicit_replay_array(ctx, &explicit, 0)
                .transpose()?.expect("explicit borrowed ids");
            assert_eq!((decoded.bytes, decoded.count, after), (ids.as_slice(), u32::from(count), explicit.len()));
            Ok(())
        });
        // Both extents are inherited, including the zero-length edge lane.
        let pair = ids.clone();
        check_visits(u64::from(count), |_| "creo replay ID traversal", |ctx| {
            let decoded = super::super::replay_affected_pair(ctx, &pair, [Some(u32::from(count)), Some(0)])
                .transpose()?.expect("inherited geometry and empty edges");
            assert_eq!((decoded.geometry_ids.bytes, decoded.geometry_ids.count), (ids.as_slice(), u32::from(count)));
            assert_eq!((decoded.edge_ids.bytes, decoded.edge_ids.count, decoded.consumed), (&[][..], 0, pair.len()));
            assert_eq!(decoded.geometry_extent, super::super::ReplayExtentSource::Inherited);
            assert_eq!(decoded.edge_extent, super::super::ReplayExtentSource::Inherited);
            Ok(())
        });
    }
}

fn empty_candidate_row(length: usize) -> FeatureRow {
    FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 100,
        body: vec![0xff; length].try_into().expect("two-byte row"),
        body_offset: 102,
        offset: 100,
    }
}

#[test]
fn replay_candidate_scans_visit_only_present_bytes_and_windows() {
    for length in 2..=5usize {
        let row = empty_candidate_row(length);
        for suffix in 0..=length {
            check_visits(suffix as u64, |_| "creo explicit replay array traversal", |ctx| {
                assert!(super::super::explicit_replay_pair_before_suffix(ctx, &row, suffix).transpose()?.is_none());
                Ok(())
            });
        }
        check_visits((length - 1) as u64, |_| "creo unanchored replay suffix traversal", |ctx| {
            assert!(super::super::unique_unanchored_replay_pair(ctx, &row, [None; 2]).transpose()?.is_none());
            Ok(())
        });
    }
}

#[test]
fn loop_roster_visits_match_entries_and_their_four_fixed_tokens() {
    for count in 1..=3usize {
        let mut body = Vec::new();
        for index in 0..count {
            body.extend_from_slice(&[index as u8 + 10, 1, 2, 3, 4, 0xe3]);
        }
        // One roster visit and four admitted PSB token visits per entry.
        check_visits((5 * count) as u64, |used| {
            if used % 5 == 0 { "creo loop history roster traversal" }
            else { "creo PSB token traversal" }
        }, |ctx| {
            let parts = super::super::loop_history_prototypes(ctx, &body, 0, count)
                .transpose()?.expect("complete borrowed roster");
            let storage = parts.1;
            let entries = parts.0;
            assert_eq!(entries.len(), count);
            for (index, entry) in entries.iter().enumerate() {
                assert_eq!(entry.loop_id, index as u32 + 10);
                assert_eq!(entry.field_bytes, [&[1][..], &[2][..], &[3][..], &[4][..]]);
                assert!(matches!(entry.boundary, super::super::BorrowedHistoryBoundary::CompoundClose));
                assert_eq!((entry.offset, entry.end_offset), (6 * index, 6 * (index + 1)));
            }
            drop(entries);
            drop(storage);
            Ok(())
        });
    }
}

#[test]
fn affected_id_visit_refusals_keep_real_named_array_coverage() {
    for count in 1..=3u8 {
        let mut bytes = b"\xe0\x01geoms_affected\0\xf8".to_vec();
        bytes.push(count);
        bytes.extend(1..=count);
        let row = FeatureRow { body: bytes.try_into().expect("affected row"), ..empty_candidate_row(2) };
        let records = crate::test_support::assert_work_boundaries(
            &["creo affected ID traversal"],
            |ctx| super::super::affected_ids(ctx, std::slice::from_ref(&row)),
        );
        assert_eq!(records.len(), 1);
        assert_eq!((records[0].feature_id, records[0].kind, records[0].offset), (7, AffectedIdKind::Geometry, 102));
        assert_eq!(records[0].ids, (1..=u32::from(count)).collect::<Vec<_>>());
    }
}

#[test]
fn feature_row_fixed_returns_are_free_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let row = empty_candidate_row(2);
    let check = |refused| {
        let mut results = Vec::new();
        results.push(super::super::round_replay_short_scalar(&ctx, &[], 0, 0).map(|v| v.is_none()));
        results.push(super::super::round_replay_fixed_token_end(&ctx, &[], 0, 0).map(|v| v.is_none()));
        results.push(super::super::round_replay_fixed_token_end(&ctx, &[0x0f], 0, 1).map(|v| v == Some(1)));
        for (payload, expected) in [
            (&[][..], FeatureFieldValue::Empty),
            (&[7][..], FeatureFieldValue::CompactInt(7)),
            (&[0xf7, 7][..], FeatureFieldValue::EntityReference { entity_id: 7, terminated: false }),
            (&[0xf7, 7, 0xfb][..], FeatureFieldValue::EntityReference { entity_id: 7, terminated: true }),
            (&[0xf8, 0][..], FeatureFieldValue::CompactIntArray(Vec::new())),
        ] {
            results.push(super::super::field_value(&ctx, payload).map(|value| value == expected));
        }
        results.push(super::super::positional_datum_geometry_table_at(&ctx, &[], 0, 87).map(|v| v.is_none()));
        results.push(super::super::geometry_table_at(&ctx, &[], 0, FeatureGeometryTableKind::DatumIds(None)).map(|v| v.is_none()));
        results.push(super::super::replay_ids(&ctx, &[], 0, 1).transpose().map(|v| v.is_none()));
        results.push(super::super::replay_affected_pair(&ctx, &[], [None; 2]).transpose().map(|v| v.is_none()));
        results.push(super::super::explicit_replay_array(&ctx, &[], 0).transpose().map(|v| v.is_none()));
        results.push(super::super::explicit_replay_pair_before_suffix(&ctx, &row, 0).transpose().map(|v| v.is_none()));
        results.push(super::super::loop_history_prototypes(&ctx, &[], 0, 0).transpose().map(|v| v.is_none()));
        results.push(super::super::loop_history_token(&ctx, &[0xe0], 0).map(|v|
            v.is_some_and(|token| token.kind == crate::psb::TokenKind::Truncated(0xe0))));
        results.push(super::super::positional_surface_merge_affected_ids(&ctx, &row, [None; 3]).transpose().map(|v| v.is_none()));
        results.push(super::super::agreed_feature_affected_ids(&ctx, &[], 7, AffectedIdKind::Edges).map(|v| v.is_none()));
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("free fixed return"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after fixed feature row returns").expect_err("zero cap");
    check(true);
    assert!(matches!(super::super::unique_unanchored_replay_pair(&ctx, &row, [None; 2]),
        Some(Err(CodecError::ResourceLimit(actual))) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn sole_matching_affected_record_has_no_remainder_scan() {
    let records = [super::super::FeatureAffectedIds {
        feature_id: 7, kind: AffectedIdKind::Edges, ids: vec![10, 11], offset: 102,
    }];
    check_visits(1, |_| "creo affected ID agreement first", |ctx| {
        assert_eq!(super::super::agreed_feature_affected_ids(ctx, &records, 7, AffectedIdKind::Edges)?, Some(&[10, 11][..]));
        Ok(())
    });
}

#[test]
fn unanchored_replay_start_scan_visits_only_its_candidate_range() {
    for prefix in 0..=3usize {
        let mut bytes = vec![0xff; prefix];
        bytes.extend_from_slice(&[0xe1, 0xe1, 40, 0xe3, 0xe3, 1, 40, 0, 0xe1, 0, 0xe3]);
        let row = FeatureRow { body: bytes.try_into().expect("valid suffix row"), ..empty_candidate_row(2) };
        // L-1 suffix windows, prefix explicit-array bytes, and 1..prefix starts.
        let starts = prefix.saturating_sub(1);
        let total = row.body.len() - 1 + prefix + starts;
        check_visits(total as u64, |used| {
            if used < (prefix + 1) as u64 { "creo unanchored replay suffix traversal" }
            else if used < (2 * prefix + 1) as u64 { "creo explicit replay array traversal" }
            else if used < (2 * prefix + 1 + starts) as u64 { "creo unanchored replay start traversal" }
            else { "creo unanchored replay suffix traversal" }
        }, |ctx| {
            assert!(super::super::unique_unanchored_replay_pair(ctx, &row, [None; 2]).transpose()?.is_none());
            Ok(())
        });
    }
}

#[test]
fn sole_surface_merge_anchor_has_no_uniqueness_remainder_scan() {
    let row = FeatureRow { body: vec![0xf7, 0x80, 0x96].try_into().expect("one anchor window"), ..empty_candidate_row(2) };
    check_visits(1, |_| "creo surface merge anchor traversal", |ctx| {
        assert!(super::super::positional_surface_merge_affected_ids(ctx, &row, [None; 3]).transpose()?.is_none());
        Ok(())
    });
}

#[test]
fn revolution_fixed_prefix_search_keeps_the_64_byte_boundary_and_one_row_visit() {
    const FULL_TURN: &[u8] = &[0, 0, 0xea, 0x44, 0, 0, 0xf6, 0xf6, 0xf6, 0, 0, 0, 0];
    // A four-byte prefix can end at byte64. Starting at61 crosses that bound.
    for prefix_offset in [6usize, 60, 61] {
        let mut bytes = vec![0xff; prefix_offset];
        bytes[..6].copy_from_slice(&[0xe3, 0xf6, 0x83, 0x95, 0xe1, 2]);
        bytes.extend_from_slice(&[0x83, 0xdf, 0xf6, 0xe3]);
        bytes.extend_from_slice(FULL_TURN);
        let row = FeatureRow {
            root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
            body: bytes.try_into().expect("bounded revolution row"),
            ..empty_candidate_row(2)
        };
        check_visits(1, |_| "creo feature row traversal", |ctx| {
            let extents = super::super::revolution_extents(ctx, std::slice::from_ref(&row))?;
            if prefix_offset <= 60 {
                assert_eq!(extents, [super::super::FeatureRevolutionExtent {
                    feature_id: 7, offset: row.body_offset + prefix_offset + 4 + 2,
                }]);
            } else {
                assert!(extents.is_empty());
            }
            Ok(())
        });
    }
}
