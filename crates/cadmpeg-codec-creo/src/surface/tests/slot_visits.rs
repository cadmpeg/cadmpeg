// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{
    named_spline_scalar_slots, scalar_slots, sequential_named_local_system_slots,
    take_spline_scalars, take_spline_vectors, ScalarBodyRefusal, ScalarTokenSlot, SplineLabel,
    SurfacePrototypeFamily,
};

fn check_work(
    work: u64,
    retained: usize,
    collection: usize,
    run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let steps: Vec<_> =
        std::iter::repeat_n(1, usize::try_from(work).expect("small Work control")).collect();
    check_work_sequence(&steps, retained, collection, run);
}

fn check_work_sequence(
    steps: &[u64],
    retained: usize,
    collection: usize,
    run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    check_work_sequence_with_materialized(steps, retained, collection, 0, run);
}

fn check_work_sequence_with_materialized(
    steps: &[u64],
    retained: usize,
    collection: usize,
    materialized: usize,
    run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let work: u64 = steps.iter().sum();
    for cap in 0..=work + 1 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = u64_from_index(retained);
        policy.limits.max_collection_items = u64_from_index(collection);
        policy.limits.max_materialized_bytes = u64_from_index(materialized);
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = if cap < work {
            let CodecError::ResourceLimit(refusal) =
                run(&ctx).expect_err("actual source visit or copy")
            else {
                panic!("resource refusal");
            };
            let mut used = 0;
            let additional = steps
                .iter()
                .copied()
                .find(|additional| {
                    if used + additional > cap {
                        true
                    } else {
                        used += additional;
                        false
                    }
                })
                .expect("first source-derived charge exceeding cap");
            assert_eq!((refusal.used, refusal.additional), (used, additional));
            refusal
        } else {
            run(&ctx).expect("exact executed Work and backing costs");
            let refusal = ctx
                .charge_work_limit(cap - work + 1, "after declared slot visits")
                .expect_err("exact remaining Work");
            assert_eq!((refusal.used, refusal.additional), (work, cap - work + 1));
            refusal
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    }
}

#[test]
fn declared_scalar_slot_visits_match_encoded_tokens() {
    for count in [0_usize, 1, 7, 17, 257] {
        for lane in 0..3 {
            let body = vec![if lane == 1 { 0x10 } else { 0xe4 }; count];
            let capacity = if count == 0 { 0 } else { count.max(4) };
            let retained = if lane == 2 {
                capacity * std::mem::size_of::<ScalarTokenSlot>() + count
            } else {
                capacity * std::mem::size_of::<Option<f64>>()
            };
            let work = u64_from_index(count * if lane == 2 { 2 } else { 1 });
            check_work(work, retained, count, |ctx| {
                let cache = ScalarCache::default();
                let mut refusal = ScalarBodyRefusal::default();
                if lane == 2 {
                    let slots = named_spline_scalar_slots(
                        ctx,
                        &SurfacePrototypeFamily::Spline(SplineLabel::Spline),
                        "tangts",
                        &body,
                        count,
                        &cache,
                        &mut refusal,
                    )?
                    .expect("complete declared tokens");
                    assert_eq!(slots, vec![(Some(1.0), vec![0xe4]); count]);
                } else {
                    let slots = if lane == 0 {
                        scalar_slots(ctx, &body, count, &cache, &mut refusal)?
                    } else {
                        sequential_named_local_system_slots(
                            ctx,
                            &body,
                            count,
                            &cache,
                            &mut refusal,
                        )?
                    };
                    assert_eq!(
                        slots,
                        Some(vec![Some(if lane == 0 { 1.0 } else { 0.0 }); count])
                    );
                }
                assert!(refusal.reason().is_none());
                Ok(())
            });
        }
    }
}

#[test]
fn positional_spline_scalar_visits_stop_before_unexecuted_storage() {
    for count in [0_usize, 1, 2, 4] {
        let body = vec![0xe4; count];
        let retained = if count == 0 {
            0
        } else {
            4 * std::mem::size_of::<f64>()
        };
        check_work(u64_from_index(count), retained, count, |ctx| {
            let mut cursor = 0;
            let values = take_spline_scalars(
                ctx,
                &body,
                &mut cursor,
                count,
                "i_points",
                &ScalarCache::default(),
            )?;
            assert_eq!(values, Some(vec![1.0; count]));
            assert_eq!(cursor, count);
            Ok(())
        });
    }
    check_work(1, 0, 0, |ctx| {
        let mut cursor = 0;
        assert!(take_spline_scalars(
            ctx,
            &[0xff; 20],
            &mut cursor,
            4,
            "i_points",
            &ScalarCache::default()
        )?
        .is_none());
        assert_eq!(cursor, 0);
        Ok(())
    });
    // The complete eight-byte token exhausts the body. The second absent slot
    // is a metadata refusal and does not visit or allocate another token.
    check_work(1, 4 * std::mem::size_of::<f64>(), 1, |ctx| {
        let mut cursor = 0;
        assert!(take_spline_scalars(
            ctx,
            &[0x46, 0, 0, 0, 0, 0, 0, 0],
            &mut cursor,
            2,
            "i_points",
            &ScalarCache::default()
        )?
        .is_none());
        assert_eq!(cursor, 8);
        Ok(())
    });
}

#[test]
fn positional_spline_vector_visits_match_complete_tuples() {
    for count in [0_usize, 1, 7, 17, 257] {
        let mut body = Vec::new();
        for _ in 0..count {
            for value in [1.0_f64, 2.0, 3.0] {
                let mut raw = value.to_be_bytes();
                raw[0] = if raw[0] == 0x3f { 0x41 } else { 0x46 };
                body.extend_from_slice(&raw);
            }
        }
        let mut steps = Vec::new();
        let mut capacity = 0;
        let mut overlap = 0;
        for index in 0..count {
            steps.extend([1; 3]);
            if index == capacity {
                if capacity != 0 {
                    overlap = capacity * std::mem::size_of::<[f64; 3]>();
                    steps.push(u64_from_index(overlap));
                }
                capacity = (2 * capacity).max(4);
            }
        }
        // Candidate backing is scoped until the final complete vector is promoted.
        let bytes = capacity * std::mem::size_of::<[f64; 3]>();
        check_work_sequence_with_materialized(&steps, bytes, count, bytes + overlap, |ctx| {
            let mut cursor = 0;
            let vectors = take_spline_vectors(
                ctx,
                &body,
                &mut cursor,
                count * 3,
                "i_points",
                &ScalarCache::default(),
            )?;
            assert_eq!(vectors, Some(vec![[1.0, 2.0, 3.0]; count]));
            assert_eq!(cursor, body.len());
            Ok(())
        });
    }
}

#[test]
fn inherited_local_system_run_admits_encoded_dispatch_and_expanded_slots() {
    for count in [1_usize, 7, 17, 257] {
        let body = if count < 128 {
            vec![0xe7, u8::try_from(count).expect("direct count")]
        } else {
            vec![0xe7, 0x81, 0x01]
        }; // Two-byte compact count 257.
        let work = 1 + u64_from_index(count);
        for cap in 0..=work + 1 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes =
                u64_from_index(count.max(4) * std::mem::size_of::<Option<f64>>());
            policy.limits.max_collection_items = u64_from_index(count);
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || {
                sequential_named_local_system_slots(
                    &ctx,
                    &body,
                    count,
                    &ScalarCache::default(),
                    &mut ScalarBodyRefusal::default(),
                )
            };
            let original = if cap < work {
                let CodecError::ResourceLimit(refusal) =
                    run().expect_err("dispatch or whole inherited expansion")
                else {
                    panic!("resource refusal");
                };
                let (used, additional, operation) = if cap == 0 {
                    (0, 1, "creo local-system scalar dispatch")
                } else {
                    (
                        1,
                        u64_from_index(count),
                        "creo local-system inherited slots",
                    )
                };
                assert_eq!(
                    (refusal.used, refusal.additional, refusal.operation),
                    (used, additional, operation)
                );
                refusal
            } else {
                assert_eq!(
                    run().expect("one encoded dispatch and exact inherited expansion"),
                    Some(vec![None; count])
                );
                let refusal = ctx
                    .charge_work_limit(cap - work + 1, "after inherited slots")
                    .expect_err("exact remaining Work");
                assert_eq!((refusal.used, refusal.additional), (work, cap - work + 1));
                refusal
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        }
    }
    // Invalid transitions execute only the one encoded-token dispatch.
    for body in [&[0xe7][..], &[0xe7, 0], &[0xe7, 2]] {
        check_work(1, 4 * std::mem::size_of::<Option<f64>>(), 1, |ctx| {
            assert!(sequential_named_local_system_slots(
                ctx,
                body,
                1,
                &ScalarCache::default(),
                &mut ScalarBodyRefusal::default()
            )?
            .is_none());
            Ok(())
        });
    }
}

#[test]
fn spline_continuation_visits_preserve_token_interiors_and_implicit_zero() {
    for name in ["i_pnts", "i_points"] {
        check_work(
            3,
            4 * std::mem::size_of::<ScalarTokenSlot>() + 1,
            2,
            |ctx| {
                let slots = named_spline_scalar_slots(
                    ctx,
                    &SurfacePrototypeFamily::Spline(SplineLabel::Spline),
                    name,
                    &[0xe4, 0xf9, 0x00],
                    2,
                    &ScalarCache::default(),
                    &mut ScalarBodyRefusal::default(),
                )?;
                assert_eq!(
                    slots,
                    Some(vec![(Some(1.0), vec![0xe4]), (Some(0.0), Vec::new())])
                );
                Ok(())
            },
        );
    }
    // The unresolved seven-byte token owns all of its embedded markers.
    let body = [0xaa, 0xf9, 0x00, 0xe3, 0xe4, 0x18, 0xe0, 0xe4];
    check_work_sequence(
        &[1, 7, 1, 1],
        4 * std::mem::size_of::<ScalarTokenSlot>() + 8,
        2,
        |ctx| {
            let slots = named_spline_scalar_slots(
                ctx,
                &SurfacePrototypeFamily::Spline(SplineLabel::Spline),
                "tangts",
                &body,
                2,
                &ScalarCache::default(),
                &mut ScalarBodyRefusal::default(),
            )?;
            assert_eq!(
                slots,
                Some(vec![(None, body[..7].to_vec()), (Some(1.0), vec![0xe4])])
            );
            Ok(())
        },
    );
}

#[test]
fn scalar_slot_free_results_return_without_work() {
    check_work(0, 0, 0, |ctx| {
        let cache = ScalarCache::default();
        let mut refusal = ScalarBodyRefusal::default();
        assert_eq!(
            scalar_slots(ctx, &[], 0, &cache, &mut refusal)?,
            Some(Vec::new())
        );
        assert_eq!(
            named_spline_scalar_slots(
                ctx,
                &SurfacePrototypeFamily::Plane,
                "tangts",
                &[],
                0,
                &cache,
                &mut refusal
            )?,
            Some(Vec::new())
        );
        assert_eq!(
            sequential_named_local_system_slots(ctx, &[], 0, &cache, &mut refusal)?,
            Some(Vec::new())
        );
        assert_eq!(
            take_spline_scalars(ctx, &[], &mut 0, 0, "i_points", &cache)?,
            Some(Vec::new())
        );
        assert_eq!(
            take_spline_scalars(ctx, &[], &mut 1, 0, "i_points", &cache)?,
            None
        );
        assert_eq!(
            take_spline_scalars(ctx, &[], &mut 0, 1, "i_points", &cache)?,
            None
        );
        assert_eq!(
            take_spline_vectors(ctx, &[], &mut 0, 0, "i_points", &cache)?,
            Some(Vec::new())
        );
        assert_eq!(
            take_spline_vectors(ctx, &[0xe4], &mut 0, 1, "i_points", &cache)?,
            None
        );
        Ok(())
    });
}
