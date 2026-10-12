// SPDX-License-Identifier: Apache-2.0

#[test]
fn shared_knot_checks_preserve_prefix_order_and_original_refusal() {
    use crate::geometry::nurbs::NurbsError;
    use cadmpeg_core::decode::ResourceDimension;

    for prefix in ["", "u_", "v_"] {
        for (knots, message) in [
            (
                vec![f64::NAN, 1.0, 0.0, 1.0],
                format!("{prefix}knots contains a non-finite value"),
            ),
            (
                vec![0.0, 1.0, 0.0, 1.0],
                format!("{prefix}knots must be non-decreasing"),
            ),
        ] {
            let standard = crate::geometry::nurbs::require_nondecreasing_knots(
                &crate::geometry::nurbs::StandardNurbsAdmission,
                &knots,
                prefix,
            );
            assert_eq!(standard, Err(NurbsError::Structure(message.clone())));
            let ctx = cadmpeg_test_support::service_decode_context();
            let admitted =
                crate::geometry::nurbs::require_nondecreasing_knots(&ctx, &knots, prefix);
            assert!(
                matches!(admitted, Err(crate::geometry::nurbs::admitted::ConstructionError::Geometry(NurbsError::Structure(text))) if text == message)
            );
            assert!(ctx.finish_session().is_ok());
        }
        for operation in ["IR NURBS knot finiteness", "IR NURBS knot order"] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    crate::geometry::tests::budget::with_policy(
                        ResourceDimension::WorkUnits,
                        cap,
                        {
                            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                            policy.limits.max_retained_bytes = 0;
                            policy.limits.max_collection_items = 0;
                            policy
                        },
                        |ctx| {
                            crate::geometry::nurbs::admitted::finish(
                                crate::geometry::nurbs::require_nondecreasing_knots(
                                    ctx,
                                    &[0.0, 0.0, 1.0, 1.0],
                                    prefix,
                                ),
                            )
                        },
                    )
                },
            );
        }
    }
}

#[test]
fn knot_constructors_share_work_keep_storage_and_preserve_refusal() {
    use crate::geometry::nurbs::{KnotVector, NurbsError};
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Four finite knots and three adjacent pairs.
    policy.limits.max_work_units = 7;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let values = vec![0.0, 0.0, 1.0, 1.0];
    let storage = values.as_ptr();
    let knots = KnotVector::new(&ctx, values)
        .expect("two admitted scans")
        .expect("ordered knots");
    assert_eq!(knots.as_ptr(), storage);
    let retained = crate::geometry::nurbs::admit_knots(&ctx, knots, "")
        .unwrap_or_else(|_| panic!("admitted knots must keep storage without admission"));
    assert_eq!(retained.as_ptr(), storage);
    let Err(CodecError::ResourceLimit(limit)) = KnotVector::new(&ctx, vec![0.0; 4]) else {
        panic!("second construction must use the exhausted account");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "IR NURBS knot finiteness");
    assert_eq!(limit.used, 7);
    assert_eq!(limit.additional, 1);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );

    for (dimension, operation) in [
        (ResourceDimension::RetainedBytes, "IR finite knot values"),
        (ResourceDimension::CollectionItems, "IR finite knot values"),
        (ResourceDimension::WorkUnits, "IR finite knot values"),
        (ResourceDimension::WorkUnits, "IR NURBS knot order"),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            crate::geometry::tests::budget::with_limit(dimension, cap, |ctx| {
                KnotVector::from_finite_lanes(ctx, vec![FiniteReal::ZERO; 4])
            })
        });
    }

    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        KnotVector::new(&ctx, vec![0.0, 2.0, 1.0, f64::NAN]).expect("diagnostic admitted"),
        Err(NurbsError::Structure(
            "knots contains a non-finite value".into()
        ))
    );
    assert_eq!(
        KnotVector::new(&ctx, vec![0.0, 2.0, 1.0]).expect("diagnostic admitted"),
        Err(NurbsError::Structure("knots must be non-decreasing".into()))
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) = KnotVector::new(&ctx, vec![f64::NAN]) else {
        panic!("diagnostic storage refusal must stay outer");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "IR NURBS refusal text");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn knot_order_charges_only_examined_pairs_and_keeps_original_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for (knots, pairs, ordered) in [
        (vec![], 0, true),
        (vec![f64::NAN], 0, true),
        (vec![1.0, 0.0, 2.0, 3.0], 1, false),
        (vec![0.0, f64::NAN, 2.0, 3.0], 1, false),
        (vec![0.0, 0.0, 1.0, 1.0], 3, true),
    ] {
        for allowance in 0..=pairs {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = crate::geometry::nurbs::knots_nondecreasing(&knots, |count| {
                ctx.charge_work(count, "test knot order")
            });
            if allowance < pairs {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("pair visit must refuse");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.used, allowance);
                assert_eq!(original.additional, 1);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
            } else {
                assert_eq!(result.unwrap(), ordered);
                if pairs == 0 {
                    ctx.finish_session().unwrap();
                } else {
                    let Err(CodecError::ResourceLimit(original)) =
                        ctx.charge_work(1, "no unused knot visits")
                    else {
                        panic!("exact pair budget must be used");
                    };
                    assert_eq!(original.used, pairs);
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                    );
                }
            }
        }
    }
}
