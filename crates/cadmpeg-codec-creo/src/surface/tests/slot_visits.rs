// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{
    named_spline_scalar_slots, scalar_slots, sequential_named_local_system_slots,
    take_spline_scalars, take_spline_vectors, ScalarBodyRefusal, SplineLabel,
    SurfacePrototypeFamily,
};

fn check_work(run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>) {
    super::work_output(run);
}

#[test]
fn declared_scalar_slot_visits_match_encoded_tokens() {
    for count in [0_usize, 1, 7, 17, 257] {
        for lane in 0..3 {
            let body = vec![if lane == 1 { 0x10 } else { 0xe4 }; count];
            check_work(|ctx| {
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

        check_work(|ctx| {
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
    check_work(|ctx| {
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
    check_work(|ctx| {
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
        check_work(|ctx| {
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
        check_work(|ctx| {
            let values = sequential_named_local_system_slots(
                ctx,
                &body,
                count,
                &ScalarCache::default(),
                &mut ScalarBodyRefusal::default(),
            )?;
            assert_eq!(values, Some(vec![None; count]));
            Ok(())
        });
    }

    // Invalid transitions execute only the one encoded-token dispatch.
    for body in [&[0xe7][..], &[0xe7, 0], &[0xe7, 2]] {
        check_work(|ctx| {
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
        check_work(|ctx| {
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
        });
    }
    // The unresolved seven-byte token owns all of its embedded markers.
    let body = [0xaa, 0xf9, 0x00, 0xe3, 0xe4, 0x18, 0xe0, 0xe4];
    check_work(|ctx| {
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
    });
}

#[test]
fn scalar_slot_free_results_preserve_original_refusal() {
    check_work(|ctx| {
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
    // Call every free route after the same original refusal, including invalid
    // metadata. A prior helper error must not hide a later route's result.
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let original = ctx
        .charge_work_limit(1, "original declared-slot refusal")
        .expect_err("original");
    let cache = ScalarCache::default();
    let mut refusal = ScalarBodyRefusal::default();
    assert!(
        matches!(scalar_slots(&ctx, &[], 0, &cache, &mut refusal), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(named_spline_scalar_slots(&ctx, &SurfacePrototypeFamily::Plane, "tangts", &[], 0, &cache, &mut refusal), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(sequential_named_local_system_slots(&ctx, &[], 0, &cache, &mut refusal), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(take_spline_scalars(&ctx, &[], &mut 1, 0, "i_points", &cache), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(take_spline_scalars(&ctx, &[], &mut 0, 1, "i_points", &cache), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(take_spline_vectors(&ctx, &[0xe4], &mut 0, 1, "i_points", &cache), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
}
