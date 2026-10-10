// SPDX-License-Identifier: Apache-2.0

use super::super::{FeatureFieldValue, FeatureGeometryTableKind, FeatureRow};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn exact_work(
    steps: &[(&'static str, u64)],
    call: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let total: u64 = steps.iter().map(|(_, units)| units).sum();
    let operations = steps
        .iter()
        .map(|(operation, _)| *operation)
        .collect::<Vec<_>>();
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &operations,
        |allowed| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = call(&ctx);
            match result {
                Ok(()) => {
                    assert_eq!(ctx.resource_refusal(), None);
                    let refusal = ctx
                        .charge_work_limit(1, "measure present row work")
                        .expect_err("measurement");
                    assert_eq!((refusal.used, refusal.additional), (total, 1));
                    assert!(
                        matches!(call(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
                    );
                    Ok(())
                }
                Err(CodecError::ResourceLimit(refusal)) => {
                    assert_eq!(ctx.resource_refusal(), Some(refusal));
                    assert!(
                        matches!(call(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
                    );
                    Err(CodecError::ResourceLimit(refusal))
                }
                Err(error) => Err(error),
            }
        },
    );
}

#[test]
fn compact_integer_eof_keeps_raw_bytes_without_visiting_absent_entries() {
    for (payload, visits) in [
        (&[0xf8, 1][..], 0usize),
        (&[0xf8, 3, 0x80, 0x80][..], 1),
        (&[0xf8, 3, 1, 0x80, 0x80][..], 2),
    ] {
        let mut steps = vec![("creo compact integer field traversal", 1); visits];
        // Failed compact arrays preserve their complete original bytes exactly once.
        steps.push((
            "creo feature raw field",
            u64::try_from(payload.len()).expect("copy bytes"),
        ));
        exact_work(&steps, |ctx| {
            assert_eq!(
                super::super::field_value(ctx, payload)?,
                FeatureFieldValue::Raw(payload.to_vec())
            );
            Ok(())
        });
    }
}

#[test]
fn datum_eof_skips_absent_entries_and_still_visits_present_invalid_prefixes() {
    for (body, visits) in [
        (&[0xf8, 1, 0xf7, 87, 0xe2][..], 0usize),
        (&b"\xf8\x02\xf7W\xe2\xe0\x01dtm_id\0\x01"[..], 1),
        (&[0xf8, 1, 0xf7, 87, 0xe2, 0xe0][..], 1),
    ] {
        let count = u32::from(body[1]);
        exact_work(&vec![("creo named datum traversal", 1); visits], |ctx| {
            assert_eq!(
                super::super::geometry_table_at(
                    ctx,
                    body,
                    0,
                    FeatureGeometryTableKind::DatumIds(None)
                )?,
                Some((count, 87, FeatureGeometryTableKind::DatumIds(None)))
            );
            Ok(())
        });
    }
    for body in [
        &[0xf8, 2, 0xf7, 87, 0xe2, 0xf7, 88, 0x80, 0x80, 0xf6][..],
        &[0xf8, 1, 0xf7, 87, 0xe2, 0xff][..],
    ] {
        exact_work(&[("creo positional datum traversal", 1)], |ctx| {
            assert!(super::super::positional_datum_geometry_table_at(ctx, body, 0, 87)?.is_none());
            Ok(())
        });
    }
}

#[test]
fn replay_ids_do_not_visit_a_second_entry_after_one_two_byte_value_reaches_eof() {
    let run = [0x80, 0x80];
    exact_work(&[("creo replay ID traversal", 1)], |ctx| {
        assert!(super::super::replay_ids(ctx, &run, 2, 0)
            .transpose()?
            .is_none());
        Ok(())
    });
}

#[test]
fn loop_roster_eof_has_only_complete_entry_work_and_preserves_invalid_present_visit() {
    for (body, visits) in [
        (&[10, 1, 2, 3, 4, 0xe3][..], 1usize),
        (&[10, 1, 2, 3, 4, 0xe3, 0xff][..], 2),
    ] {
        let mut steps = vec![("creo loop history roster traversal", 1)];
        steps.extend([("creo PSB token traversal", 1); 4]);
        if visits == 2 {
            steps.push(("creo loop history roster traversal", 1));
        }
        exact_work(&steps, |ctx| {
            assert!(super::super::loop_history_prototypes(ctx, body, 0, 2)
                .transpose()?
                .is_none());
            Ok(())
        });
    }
}

#[test]
fn affected_id_eof_has_one_present_value_and_the_same_search_extent_as_complete_lane() {
    const LABELS: &[&[u8]] = &[
        b"geoms_affected",
        b"edgs_affected",
        b"strong_parents",
        b"parent_table",
        b"contours",
        b"qlts_affected",
    ];
    for count in [1u8, 2] {
        let mut body = b"\xe0\x01geoms_affected\0\xf8".to_vec();
        body.extend_from_slice(&[count, 0x80, 0x80]);
        let length = body.len();
        let row = FeatureRow {
            feature_id: 7,
            root_schema_class: None,
            stream_offset: 100,
            body_offset: 102,
            offset: 100,
            body: body.try_into().expect("complete row header"),
        };
        // Core find_bytes charges haystack + needle for each search. Both rows have
        // the same bytes and search ranges apart from their fixed count byte.
        let from = 2 + LABELS[0].len() + 1;
        let mut steps = vec![
            ("creo feature row traversal", 1),
            (
                "find Creo feature row",
                u64::try_from(length + LABELS[0].len()).expect("first search"),
            ),
            ("creo affected ID traversal", 1),
            (
                "find Creo feature row",
                u64::try_from(length - from + LABELS[0].len()).expect("remainder search"),
            ),
        ];
        for label in &LABELS[1..] {
            steps.push((
                "find Creo feature row",
                u64::try_from(length + label.len()).expect("label search"),
            ));
        }
        exact_work(&steps, |ctx| {
            let records = super::super::affected_ids(ctx, std::slice::from_ref(&row))?;
            if count == 1 {
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].ids, [128]);
                assert_eq!(records[0].feature_id, 7);
                assert_eq!(records[0].offset, 102);
            } else {
                assert!(records.is_empty());
            }
            Ok(())
        });
    }
}

#[test]
fn replay_scalar_fallback_token_has_one_psb_visit() {
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo PSB token traversal"],
        |work| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            assert!(super::super::round_replay_short_scalar(&ctx, &[0xf7, 1], 0, 2)?.is_none());
            let limit = ctx
                .charge_work_limit(u64::MAX, "measure replay scalar visits")
                .expect_err("measure completed traversal");
            assert_eq!(limit.used, 1);
            Ok::<_, CodecError>(())
        },
    );
}
