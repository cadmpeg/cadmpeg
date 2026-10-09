// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::refusal_probe::RefusalProbe;

use super::*;

#[test]
fn compressed_global_cards_refuse_only_the_first_executed_card_step() {
    let mut card = [b' '; 80];
    card[..7].copy_from_slice(b"1H,,1H;");
    for count in [1_usize, 3] {
        let cards = [&card[..]; 3];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "iges_compressed_global_stream",
            None,
        );
        let result = logical_global_stream(&cards[..count], &ctx);
        let refusal = match result.as_ref() {
            Err(CodecError::ResourceLimit(refusal)) => *refusal,
            _ => panic!("expected first Global card-step refusal"),
        };
        drop((result, probe));
        // The prior width fold visits each card exactly once.
        assert_eq!(refusal.used, u64::try_from(count).unwrap());
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(original)) if original == refusal
        ));
        // Width fold: n. Card steps: n. Two one-byte UTF-8 checks and
        // two one-byte integer parses per card: 4*n. No empty card read.
        let required = u64::try_from(6 * count).unwrap();
        for cap in [required - 1, required] {
            let arena = DecodeArena::new();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = logical_global_stream(&cards[..count], &ctx);
            if cap < required {
                let refusal = match result.as_ref() {
                    Err(CodecError::ResourceLimit(refusal)) => *refusal,
                    _ => panic!("expected final Global count parse refusal"),
                };
                drop(result);
                assert_eq!(refusal.operation, "iges compressed Global Hollerith number");
                assert_eq!((refusal.used, refusal.additional), (required - 1, 1));
                assert!(matches!(
                    ctx.finish_session(),
                    Err(CodecError::ResourceLimit(original)) if original == refusal
                ));
            } else {
                let result = result.unwrap();
                assert_eq!(result.0, b"1H,,1H;".repeat(count));
                drop(result);
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn compressed_parameter_lines_refuse_one_step_before_a_malformed_first_line() {
    let first = b"D1@1_116@3_0@4_0@5_0@6_0@7_0@8_0@9_00000000".as_slice();
    let last = b"@12_0@13_0@14_3@15_0@16_@17_@18_POINT@19_0;".as_slice();
    let invalid = [b'X'; 65];
    let lines = [first, last, &invalid, b"unused", b"unused"];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "iges_compressed_parameter_lines",
        None,
    );
    let refusal = match parse_data_entity(&lines, 0, None, b',', b';', &ctx) {
        Err(CodecError::ResourceLimit(refusal)) => refusal,
        _ => panic!("expected first Parameter Data line-step refusal"),
    };
    drop(probe);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        parse_data_entity(&lines, 0, None, b',', b';', &ctx),
        Err(CodecError::ResourceLimit(original)) if original == refusal
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(original)) if original == refusal
    ));
}

#[test]
fn compressed_normalization_refuses_one_step_for_each_fallible_output_pass() {
    let source = compressed_points_file();
    for operation in [
        "iges_compressed_parameter_starts",
        "iges compressed Directory cards",
        "iges compressed Parameter Data cards",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let refusal = match normalize(&source, &ctx) {
            Err(CodecError::ResourceLimit(refusal)) => refusal,
            _ => panic!("expected first output-pass step refusal"),
        };
        drop(probe);
        assert_eq!(refusal.additional, 1);
        assert_eq!(refusal.operation, operation);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert!(matches!(
            normalize(&source, &ctx),
            Err(CodecError::ResourceLimit(original)) if original == refusal
        ));
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(original)) if original == refusal
        ));
    }
}

#[test]
fn compressed_parameter_card_writer_admits_actual_lines_and_observes_empty_fuse() {
    let fields = DirectoryFields(std::array::from_fn(|_| std::rc::Rc::new(Vec::new())));
    let lines = [b"116,0,0,0;".as_slice(); 3];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let entity = DataEntity { sequence: 1, fields, parameter_lines: &lines };
    let probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "iges compressed Parameter Data cards",
        None,
    );
    let mut output = Vec::new();
    let refusal = match super::super::append_parameter_cards(&mut output, &entity, 1, &ctx) {
        Err(CodecError::ResourceLimit(refusal)) => refusal,
        _ => panic!("expected first Parameter card-step refusal"),
    };
    drop(probe);
    assert_eq!((refusal.used, refusal.additional), (0, 1));
    assert!(output.is_empty());
    let empty = DataEntity { parameter_lines: &[], ..entity };
    assert!(matches!(
        super::super::append_parameter_cards(&mut output, &empty, 1, &ctx),
        Err(CodecError::ResourceLimit(original)) if original == refusal
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(original)) if original == refusal
    ));
}


fn hollerith_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    policy
}

#[test]
fn compressed_hollerith_digits_admit_only_existing_bytes() {
    for count in [1_usize, 64] {
        let bytes = vec![b'1'; count];
        let required = u64::try_from(count).unwrap();
        for cap in [0, required - 1, required] {
            let arena = DecodeArena::new();
            let policy = hollerith_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = hollerith_at(&bytes, 0, &ctx);
            if cap == required {
                assert!(result.unwrap().is_none());
                ctx.finish_session().unwrap();
                continue;
            }
            let Err(CodecError::ResourceLimit(first)) = result else {
                panic!("expected first or last actual digit refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "iges compressed Global Hollerith digits");
            assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
            for input in [b"".as_slice(), bytes.as_slice()] {
                assert!(matches!(hollerith_at(input, 0, &ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}

#[test]
fn compressed_hollerith_absent_marker_is_free_after_the_final_byte() {
    let arena = DecodeArena::new();
    let policy = hollerith_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        for (bytes, start) in [(b"".as_slice(), 0), (b"1".as_slice(), 1),
            (b"1".as_slice(), usize::MAX)] {
            assert!(hollerith_at(bytes, start, &ctx).unwrap().is_none());
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn compressed_hollerith_marker_and_payload_keep_their_exact_bounds() {
    for (bytes, expected) in [(b"0H".as_slice(), (2, 2)),
        (b"1Hx".as_slice(), (2, 3)), (b"1h,".as_slice(), (2, 3))] {
        // Digit and marker visits, one UTF-8 byte, one integer-parse byte.
        for cap in [3, 4] {
            let arena = DecodeArena::new();
            let policy = hollerith_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = hollerith_at(bytes, 0, &ctx);
            if cap == 4 {
                assert_eq!(result.unwrap(), Some(expected));
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(first)) = result else {
                    panic!("expected count-parse refusal");
                };
                assert_eq!(first.operation, "iges compressed Global Hollerith number");
                assert_eq!((first.limit, first.used, first.additional), (3, 3, 1));
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
    let arena = DecodeArena::new();
    let policy = hollerith_policy(4);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(hollerith_at(b"1H", 0, &ctx), Err(CodecError::Malformed(_))));
    ctx.finish_session().unwrap();
}

#[test]
fn compressed_hollerith_empty_routes_preserve_each_original_refusal() {
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes, ResourceDimension::RetainedBytes,
        ResourceDimension::Entities, ResourceDimension::RecursionDepth] {
        let arena = DecodeArena::new();
        let policy = hollerith_policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let refused = match dimension {
            ResourceDimension::WorkUnits => ctx.charge_work(1, "test original Hollerith refusal"),
            ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original Hollerith refusal"),
            ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original Hollerith refusal").map(|_| ()),
            ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original Hollerith refusal"),
            ResourceDimension::Entities => ctx.charge_entities(1, "test original Hollerith refusal"),
            ResourceDimension::RecursionDepth => ctx.enter_nested("test original Hollerith refusal").map(|_| ()),
            _ => panic!("Hollerith refusal dimension"),
        };
        let Err(CodecError::ResourceLimit(first)) = refused else { panic!("expected original refusal"); };
        assert_eq!(first.dimension, dimension);
        for _ in 0..64 {
            for (bytes, start) in [(b"".as_slice(), 0), (b"1".as_slice(), 1),
                (b"1".as_slice(), usize::MAX), (b"1Hx".as_slice(), 0)] {
                assert!(matches!(hollerith_at(bytes, start, &ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}
