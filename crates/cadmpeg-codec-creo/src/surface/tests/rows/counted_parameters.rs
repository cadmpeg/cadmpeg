// SPDX-License-Identifier: Apache-2.0
use crate::scalar;

#[test]
fn counted_parameter_state_resize_refuses_work() {
    let cache = scalar::ScalarCache::default();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo_counted_parameter_slots",
        |ctx| crate::surface::counted_parameter_scalar_slots(ctx, &[0xe4], 1, &cache),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo_counted_parameter_slots")
    );
}

#[test]
fn counted_parameter_state_cost_counts_map_entries_and_active_parse() {
    use crate::surface::{CountedParameterParse, CountedParameterState};
    use cadmpeg_core::decode::cost::DecodeCost;
    crate::decode::with_test_decode_ctx(|ctx| {
        let empty = CountedParameterState(std::collections::BTreeMap::new());
        assert_eq!(empty.decode_cost(ctx, "counted parameter state cost")?, 0);
        let state = CountedParameterState(std::collections::BTreeMap::from([
            (0, CountedParameterParse::Ambiguous),
            (
                1,
                CountedParameterParse::Unique(vec![(Some(1.0), vec![0xe4])]),
            ),
        ]));
        // Two keys, two parse tags, one option tag, one scalar and one source byte.
        let expected =
            2 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>()) + 2 + 1 + 8 + 1;
        assert_eq!(
            state.decode_cost(ctx, "counted parameter state cost")?,
            expected
        );
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .expect("state costs fit service work");
}

#[test]
fn counted_parameter_empty_extents_are_free_and_preserve_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let cache = scalar::ScalarCache::default();
    // Exactly zero slots consume exactly zero bytes. A nonempty body with
    // count zero and an empty body with positive count have no complete parse.
    let cases: &[(&[u8], usize, bool)] = &[
        (&[], 0, true), (&[0xe4], 0, false), (&[0xe5, 0xe6], 0, false),
        (&[], 1, false), (&[], usize::MAX, false),
    ];
    for &(body, count, complete) in cases {
        assert_eq!(crate::surface::counted_parameter_scalar_slots(&ctx, body, count, &cache)
            .expect("extent needs no state table"), complete.then(Vec::new));
    }
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after empty counted parameter extent")
        .expect_err("zero work cap");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1));
    for _ in 0..2 {
        for &(body, count, _) in cases {
            assert!(matches!(crate::surface::counted_parameter_scalar_slots(&ctx, body, count, &cache),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn counted_parameter_ambiguous_continuation_does_not_construct_suffix() {
    use crate::surface::{advance_counted_parameter_parse, CountedParameterParse};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let invoked = std::cell::Cell::new(false);
    for token in [0xe4, 0xe5, 0xe6] {
        let advanced = advance_counted_parameter_parse(&ctx, CountedParameterParse::Ambiguous, || {
            invoked.set(true);
            crate::surface::counted_parameter_suffix(&ctx, Some(0.0), &[token])
        }).expect("ambiguous state has no suffix storage");
        assert!(matches!(advanced, CountedParameterParse::Ambiguous));
        assert!(!invoked.get());
    }
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after ambiguous counted parameter continuation")
        .expect_err("zero work cap");
    for _ in 0..2 {
        assert!(matches!(advance_counted_parameter_parse(&ctx, CountedParameterParse::Ambiguous, || {
            invoked.set(true);
            crate::surface::counted_parameter_suffix(&ctx, Some(0.0), &[0xe4])
        }), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(!invoked.get());
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn counted_parameter_ambiguity_survives_following_scalar_and_zero_runs() {
    let cache = scalar::ScalarCache::from_section(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
    // The first nine bytes have two complete five-slot tokenizations. A common
    // continuation cannot make those earlier tokenizations unique.
    for (suffix, additional) in [(0xe4, 1), (0xe5, 2), (0xe6, 3)] {
        let body = [0x18, 0, 0xe5, 0x29, 0x18, 4, 0x29, 5, 0xe6, suffix];
        assert_eq!(super::super::counted_parameter_scalar_slots(&body, 5 + additional, &cache), None);
    }
}
