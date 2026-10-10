// SPDX-License-Identifier: Apache-2.0
use super::counted_parameter_scalar_slots;
use crate::scalar;
use crate::surface::SurfaceNamedValue;

#[test]
fn counted_parameters_expand_compact_zero_runs() {
    let body = [0xe4, 0xe5, 0x0f, 0xe6];

    assert_eq!(
        counted_parameter_scalar_slots(&body, 7, &scalar::ScalarCache::default()),
        Some(vec![
            (Some(1.0), vec![0xe4]),
            (Some(0.0), vec![0xe5]),
            (Some(0.0), vec![]),
            (Some(0.0), vec![0x0f]),
            (Some(0.0), vec![0xe6]),
            (Some(0.0), vec![]),
            (Some(0.0), vec![]),
        ])
    );
}

#[test]
fn counted_parameter_slots_refuse_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let error = crate::test_support::last_refusal_at(
        &[0],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo_counted_parameter_slots",
        |ctx| {
            crate::surface::counted_parameter_scalar_slots(
                ctx,
                &[0xe4],
                1,
                &scalar::ScalarCache::default(),
            )
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo_counted_parameter_slots"
    ));
}

fn counted_slot_error(
    body: &[u8],
    count: usize,
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(body, &arena, &policy)
        .expect("counted parameter input fits the root limit");
    crate::surface::counted_parameter_scalar_slots(
        &ctx,
        body,
        count,
        &scalar::ScalarCache::default(),
    )
    .expect_err("the selected parser allocation exceeds its limit")
}

#[test]
fn counted_parameter_initial_tree_entry_refuses_before_insert() {
    let body = [0xe4];
    assert_eq!(
        counted_parameter_scalar_slots(&body, 1, &scalar::ScalarCache::default()),
        Some(vec![(Some(1.0), vec![0xe4])])
    );
    let error = counted_slot_error(
        &body,
        1,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo counted parameter initial state"),
            |cap| Err::<(), _>(counted_slot_error(&body, 1, cap, u64::MAX)),
        ),
        u64::MAX,
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo counted parameter initial state"
    ));
}

#[test]
fn counted_parameter_token_bytes_refuse_before_copy() {
    let error = counted_slot_error(
        &[0xe4],
        1,
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo counted parameter token bytes"),
            |cap| Err::<(), _>(counted_slot_error(&[0xe4], 1, u64::MAX, cap)),
        ),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo counted parameter token bytes"
    ));
}

#[test]
fn counted_parameter_next_tree_entry_refuses_before_insert() {
    let error = counted_slot_error(
        &[0xe4],
        1,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo counted parameter state entries"),
            |cap| Err::<(), _>(counted_slot_error(&[0xe4], 1, cap, u64::MAX)),
        ),
        u64::MAX,
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo counted parameter state entries"
    ));
}

#[test]
fn counted_parameter_zero_run_preserves_expanded_slots() {
    let body = [0xe5];
    assert_eq!(
        counted_parameter_scalar_slots(&body, 2, &scalar::ScalarCache::default()),
        Some(vec![(Some(0.0), vec![0xe5]), (Some(0.0), vec![])])
    );
}

#[test]
fn counted_parameter_branch_preserves_scalar_tokens() {
    let body = [0xe4, 0x18];
    assert_eq!(
        counted_parameter_scalar_slots(&body, 2, &scalar::ScalarCache::default()),
        Some(vec![(Some(1.0), vec![0xe4]), (Some(0.0), vec![0x18])])
    );
}

#[test]
fn counted_parameters_use_the_exact_extent_to_select_cache_or_zero() {
    let cache = scalar::ScalarCache::from_section(&[0x46, 0, 0, 0, 0, 0, 0, 0]);

    assert_eq!(
        counted_parameter_scalar_slots(&[0x18, 0x00], 1, &cache),
        Some(vec![(Some(2.0), vec![0x18, 0x00])])
    );
    assert_eq!(
        counted_parameter_scalar_slots(&[0x18, 0, 1, 2, 3, 4, 5, 6], 2, &cache),
        Some(vec![
            (Some(0.0), vec![0x18]),
            (
                Some(f64::from_be_bytes([0x40, 0x75, 1, 2, 3, 4, 5, 6])),
                vec![0, 1, 2, 3, 4, 5, 6],
            ),
        ])
    );
}

#[test]
fn counted_parameters_reject_multiple_complete_tokenizations() {
    let cache = scalar::ScalarCache::from_section(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
    let body = [0x18, 0, 0xe5, 0x29, 0x18, 4, 0x29, 5, 0xe6];

    assert_eq!(counted_parameter_scalar_slots(&body, 5, &cache), None);
}

#[test]
fn counted_parameters_require_exact_zero_run_cardinality() {
    let body = [0xe4, 0xe5, 0x0f, 0xe6];

    assert_eq!(
        counted_parameter_scalar_slots(&body, 6, &scalar::ScalarCache::default()),
        None
    );
    assert_eq!(
        counted_parameter_scalar_slots(&body, 8, &scalar::ScalarCache::default()),
        None
    );
}

#[test]
fn u_params_refuses_collection_limit_before_allocating_slots() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = b"srf_prim_ptr(splsrf)\0\xe0\x02u_params\0\xf8\x04\x0f\x0f\x0f\x0f\xe3";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = last_limit_before_counted_scalar_array(payload);
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("small prototype payload is admitted");
    let error = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("four scalar slots exceed the three-item limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar array values"
    ));

    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("small prototype payload is admitted");
    let records = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service profile admits four slots");
    let Some(SurfaceNamedValue::CountedScalarArray(array)) =
        records[0].field("u_params").map(|field| &field.value)
    else {
        panic!("u_params must decode as a counted array");
    };
    assert_eq!(array.values(), &[Some(0.0); 4]);
}

#[test]
fn v_params_refuses_collection_limit_before_allocating_slots() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = b"srf_prim_ptr(splsrf)\0\xe0\x02v_params\0\xf8\x04\x0f\x0f\x0f\x0f\xe3";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = last_limit_before_counted_scalar_array(payload);
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("small prototype payload is admitted");
    let error = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("four scalar slots exceed the three-item limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar array values"
    ));

    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("small prototype payload is admitted");
    let records = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service profile admits four slots");
    let Some(SurfaceNamedValue::CountedScalarArray(array)) =
        records[0].field("v_params").map(|field| &field.value)
    else {
        panic!("v_params must decode as a counted array");
    };
    assert_eq!(array.values(), &[Some(0.0); 4]);
}

#[test]
fn params_refuses_collection_limit_before_allocating_slots() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = b"srf_prim_ptr(tab_cyl)\0\xe0\x02params\0\xf8\x03\xe6\xe3";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = last_limit_before_counted_scalar_array(payload);
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("small prototype payload is admitted");
    let error = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("three scalar slots exceed the two-item limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar array values"
    ));

    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("small prototype payload is admitted");
    let records = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service profile admits three slots");
    let Some(SurfaceNamedValue::CountedScalarArray(array)) =
        records[0].field("params").map(|field| &field.value)
    else {
        panic!("params must decode as a counted array");
    };
    assert_eq!(array.values(), &[Some(0.0); 3]);
}

#[test]
fn counted_surface_arrays_share_the_collection_item_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let payload = b"srf_prim_ptr(splsrf)\0\
        \xe0\x02u_params\0\xf8\x02\x0f\x0f\
        \xe0\x02v_params\0\xf8\x02\x0f\x0f\xe3";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = last_limit_before_counted_scalar_array(payload);
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("small prototype payload is admitted");
    let error = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("the second array exceeds the shared collection budget");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo scalar array values"
    ));

    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &service)
        .expect("small prototype payload is admitted");
    let records = crate::surface::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service profile admits both arrays");
    for name in ["u_params", "v_params"] {
        let Some(SurfaceNamedValue::CountedScalarArray(array)) =
            records[0].field(name).map(|field| &field.value)
        else {
            panic!("{name} must decode as a counted array");
        };
        assert_eq!(array.values(), &[Some(0.0); 2]);
    }
}

fn last_limit_before_counted_scalar_array(payload: &[u8]) -> u64 {
    use cadmpeg_core::decode::ResourceDimension;
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        "creo scalar array values",
        |ctx| {
            crate::surface::named_prototype_records(
                ctx,
                payload,
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("scalar array boundary must refuse");
    };
    refusal.limit
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
fn counted_parameter_ambiguity_survives_following_scalar_and_zero_runs() {
    let cache = scalar::ScalarCache::from_section(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
    // The first nine bytes have two complete five-slot tokenizations. A common
    // continuation cannot make those earlier tokenizations unique.
    for (suffix, additional) in [(0xe4, 1), (0xe5, 2), (0xe6, 3)] {
        let body = [0x18, 0, 0xe5, 0x29, 0x18, 4, 0x29, 5, 0xe6, suffix];
        assert_eq!(super::super::counted_parameter_scalar_slots(&body, 5 + additional, &cache), None);
    }
}
