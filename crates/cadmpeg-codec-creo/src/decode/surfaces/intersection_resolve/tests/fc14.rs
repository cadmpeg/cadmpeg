// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn record() -> crate::curve::FcCurveCoordinates {
    crate::curve::FcCurveCoordinates {
        curve_id: 77,
        subtype: 0x14,
        body: Vec::new(),
        values_mm: Vec::new(),
        tokens: vec![
            crate::curve::FcCurveCoordinateToken {
                value_mm: -3.0,
                raw: vec![0x2d, 0x08, 0, 0, 0, 0, 0, 0],
                offset: 0,
            };
            4
        ],
        opaque_spans: Vec::new(),
        offset: 0,
    }
}

#[test]
fn fc14_fixed_token_comparison_uses_four_visits_and_no_storage() {
    let record = record();
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::super::fc14_held_coordinate(&ctx, Some(&record))
    };
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo FC14 coordinate tokens"; 4],
        run,
    );
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo FC14 coordinate tokens",
        |ctx| super::super::fc14_held_coordinate(ctx, Some(&record)),
    );
    let CodecError::ResourceLimit(last) = error else {
        panic!("work refusal");
    };
    assert_eq!((last.used, last.additional), (3, 1));
    let below = last.limit;
    // Refusal location comes from the walker; four present visits remain the exact success bound.
    for cap in [below, 4] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = super::super::fc14_held_coordinate(&ctx, Some(&record));
        if cap == 4 {
            assert_eq!(result.expect("four visits"), Some(-3.0));
        } else {
            let Err(CodecError::ResourceLimit(refusal)) = result else {
                panic!("fourth visit must refuse");
            };
            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
            assert_eq!(refusal.operation, "creo FC14 coordinate tokens");
            assert_eq!(refusal.used, 3);
            assert_eq!(refusal.additional, 1);
            assert!(matches!(super::super::fc14_held_coordinate(&ctx, None),
                Err(CodecError::ResourceLimit(original)) if original == refusal));
        }
    }
}

#[test]
fn fc14_stops_at_disagreement_and_does_not_visit_tail() {
    let mut record = record();
    record.tokens[1].raw[1] = 1;
    record
        .tokens
        .extend(std::iter::repeat_n(record.tokens[0].clone(), 64));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::fc14_held_coordinate(&ctx, Some(&record)).expect("two visits"),
        None
    );
}

#[test]
fn fc14_missing_and_incomplete_routes_charge_only_present_tokens() {
    for count in 0..4 {
        let mut record = record();
        record.tokens.truncate(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(count).expect("fixed count");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(
            super::super::fc14_held_coordinate(&ctx, Some(&record)).expect("actual visits"),
            None
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::fc14_held_coordinate(&ctx, None).expect("missing record"),
        None
    );
}

#[test]
fn fc14_refuses_nonfinite_values_and_non_world_token_widths() {
    for value in [f64::INFINITY, f64::NAN] {
        let mut record = record();
        for token in &mut record.tokens {
            token.value_mm = value;
        }
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                super::super::fc14_held_coordinate(ctx, Some(&record))
            })
            .expect("visited finite gate"),
            None
        );
    }
    for width in [7, 9] {
        let mut record = record();
        for token in &mut record.tokens {
            token.raw.resize(width, 0);
        }
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| {
                super::super::fc14_held_coordinate(ctx, Some(&record))
            })
            .expect("fixed width gate"),
            None
        );
    }
}

#[test]
fn fc14_ignored_tokens_each_use_one_visit() {
    let mut record = record();
    let ignored = crate::curve::FcCurveCoordinateToken {
        value_mm: 3.0,
        raw: vec![0x46, 0x08, 0, 0, 0, 0, 0, 0],
        offset: 0,
    };
    record.tokens.insert(0, ignored.clone());
    record.tokens.insert(2, ignored.clone());
    record.tokens.push(ignored);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::fc14_held_coordinate(&ctx, Some(&record)).expect("seven visits"),
        Some(-3.0)
    );
}
