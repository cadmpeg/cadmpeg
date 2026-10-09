// SPDX-License-Identifier: Apache-2.0
use super::*;

fn reflected_x() -> Transform {
    Transform::affine([[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]).unwrap()
}

#[test]
fn replica_offset_uses_the_placed_cylinder_chart_cross_once() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cylinder", cylinder());
    let placed = procedural(&mut ir, "reflected", ProceduralSurfaceDefinition::Replica {
        source: base.clone(), transform: reflected_x(),
    });
    let shifted = offset(&mut ir, "outer", placed, 1.0);
    let inner = offset(&mut ir, "inner", base, 1.0);
    let placed_inner = procedural(&mut ir, "reflected-inner", ProceduralSurfaceDefinition::Replica {
        source: inner, transform: reflected_x(),
    });
    let index = ModelIndex::build(&ir, StandardIndex);
    for request in [SurfaceRequest::First, SurfaceRequest::Second] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
            // T C=(-2cos(u),2sin(u),v), chart normal=(cos(u),-sin(u),0).
            // Offset(T C)=(-cos(u),sin(u),v).
            let result = model_jet(admission, &index, &shifted, 0.0, 0.25, request).unwrap();
            assert_eq!(result.point.get(), Point3::new(-1.0, 0.0, 0.25));
            assert_eq!(result.first.unwrap()[0].get(), Vector3::new(0.0, 1.0, 0.0));
            assert_eq!(result.first.unwrap()[1].get(), Vector3::new(0.0, 0.0, 1.0));
            if request == SurfaceRequest::Second {
                assert_eq!(result.second.unwrap()[0].get(), Vector3::new(1.0, 0.0, 0.0));
                assert_eq!(result.second.unwrap()[1].get(), Vector3::new(0.0, 0.0, 0.0));
                assert_eq!(result.second.unwrap()[2].get(), Vector3::new(0.0, 0.0, 0.0));
            }
            assert_eq!(crate::eval::model_surface_point_by_id(admission, &index, &shifted, 0.0, 0.25).unwrap(), result.point);
            // T Offset(C)=(-3cos(u),3sin(u),v): the real order differs.
            let inner = model_jet(admission, &index, &placed_inner, 0.0, 0.25, request).unwrap();
            assert_eq!(inner.point.get(), Point3::new(-3.0, 0.0, 0.25));
            assert_eq!(inner.first.unwrap()[0].get(), Vector3::new(0.0, 3.0, 0.0));
            if request == SurfaceRequest::Second {
                assert_eq!(inner.second.unwrap()[0].get(), Vector3::new(3.0, 0.0, 0.0));
            }
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn replica_nurbs_flags_and_subset_signs_follow_actual_placed_chart_selection() {
    for reversed in [false, true] {
        for rational in [false, true] {
            for before_replica in [false, true] {
                for senses in [[true, true], [false, true], [true, false], [false, false]] {
                    let mut ir = CadIr::empty();
                    let mut support = stored(&mut ir, "cubic", cubic(reversed, rational));
                    if before_replica {
                        support = procedural(&mut ir, "subset-before", ProceduralSurfaceDefinition::Subset(
                            SubsetSurfaceConstruction::try_new(support, [[0.0, 1.0], [0.0, 1.0]], Some(senses[0]), Some(senses[1]), None).unwrap()));
                    }
                    support = procedural(&mut ir, "replica", ProceduralSurfaceDefinition::Replica {
                        source: support, transform: reflected_x(),
                    });
                    if !before_replica {
                        support = procedural(&mut ir, "subset-after", ProceduralSurfaceDefinition::Subset(
                            SubsetSurfaceConstruction::try_new(support, [[0.0, 1.0], [0.0, 1.0]], Some(senses[0]), Some(senses[1]), None).unwrap()));
                    }
                    let shifted = offset(&mut ir, "outer", support, 1.0);
                    let index = ModelIndex::build(&ir, StandardIndex);
                    let chart_sign = if senses[0] == senses[1] { 1.0 } else { -1.0 };
                    for request in [SurfaceRequest::First, SurfaceRequest::Second] {
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_retained_bytes = 0;
                        let arena = DecodeArena::new();
                        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                        for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
                            // T S=(-u,v,u^3), n=(-3u^2,0,-1)/sqrt(1+9u^4).
                            // At zero n_u=0 and n_uu=(-6,0,0). The successful
                            // placed-cross branch ignores the stored normal flag.
                            let result = model_jet(admission, &index, &shifted, 0.0, 0.0, request).unwrap();
                            assert_eq!(result.point.get(), Point3::new(0.0, 0.0, -chart_sign));
                            close(result.first.unwrap()[0].get(), Vector3::new(if senses[0] { -1.0 } else { 1.0 }, 0.0, 0.0));
                            close(result.first.unwrap()[1].get(), Vector3::new(0.0, if senses[1] { 1.0 } else { -1.0 }, 0.0));
                            if request == SurfaceRequest::Second {
                                close(result.second.unwrap()[0].get(), Vector3::new(-6.0 * chart_sign, 0.0, 0.0));
                                close(result.second.unwrap()[1].get(), Vector3::new(0.0, 0.0, 0.0));
                                close(result.second.unwrap()[2].get(), Vector3::new(0.0, 0.0, 0.0));
                            }
                            assert_eq!(crate::eval::model_surface_point_by_id(admission, &index, &shifted, 0.0, 0.0).unwrap(), result.point);
                        }
                        ctx.finish_session().unwrap();
                    }
                }
            }
        }
    }
}

#[test]
fn singular_replica_keeps_regular_surface_orders_and_exact_zero_offset_identity() {
    for rank_two in [false, true] {
        let mut ir = CadIr::empty();
        let base = stored(&mut ir, "plane", plane());
        let placed = procedural(&mut ir, "singular", ProceduralSurfaceDefinition::Replica {
            source: base, transform: Transform::affine([
                [1.0, 0.0, 0.0, 0.0], [0.0, if rank_two { 1.0 } else { 0.0 }, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0],
            ]).unwrap(),
        });
        let shifted = offset(&mut ir, "outer", placed.clone(), 1.0);
        let zero = offset(&mut ir, "zero", placed.clone(), 0.0);
        let index = ModelIndex::build(&ir, StandardIndex);
        for request in [SurfaceRequest::First, SurfaceRequest::Second] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            for admission in [EvaluationAdmission::Decode(&ctx), EvaluationAdmission::Standard] {
                let source = model_jet(admission, &index, &placed, 0.25, 0.5, request).unwrap();
                let identity = model_jet(admission, &index, &zero, 0.25, 0.5, request).unwrap();
                assert_eq!(source.point.get(), Point3::new(0.25, if rank_two { 0.5 } else { 0.0 }, 0.0));
                assert_eq!(source.first.unwrap()[0].get(), Vector3::new(1.0, 0.0, 0.0));
                assert_eq!(source.first.unwrap()[1].get(), Vector3::new(0.0, if rank_two { 1.0 } else { 0.0 }, 0.0));
                assert_eq!(identity.point, source.point);
                assert_eq!(identity.first, source.first);
                assert_eq!(identity.second, source.second);
                assert_eq!(source.second.unwrap(), [crate::features::FiniteVector3::ZERO; 3]);
                assert_eq!(crate::eval::model_surface_point_by_id(admission, &index, &placed, 0.25, 0.5).unwrap(), source.point);
                let result = model_jet(admission, &index, &shifted, 0.25, 0.5, request);
                if rank_two {
                    let result = result.unwrap();
                    assert_eq!(result.point.get(), Point3::new(0.25, 0.5, 1.0));
                    assert_eq!(result.first, source.first);
                    assert_eq!(result.second, source.second);
                    assert_eq!(crate::eval::model_surface_point_by_id(admission, &index, &shifted, 0.25, 0.5).unwrap(), result.point);
                } else {
                    assert!(matches!(result, Err(EvaluationFailure::NoValue)));
                    // The point owner tries the inverse-normal fallback after the
                    // degenerate placed cross. This singular transform has no
                    // inverse, so that owner preserves its NonFinite result.
                    assert!(matches!(crate::eval::model_surface_point_by_id(admission, &index, &shifted, 0.25, 0.5),
                        Err(EvaluationFailure::NonFinite(point)) if point.x.is_nan() && point.y.is_nan() && point.z.is_nan()));
                }
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn nested_replica_uses_each_placed_chart_sign_once_and_keeps_original_fuse() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cylinder", cylinder());
    let first = procedural(&mut ir, "first", ProceduralSurfaceDefinition::Replica { source: base, transform: reflected_x() });
    let second = procedural(&mut ir, "second", ProceduralSurfaceDefinition::Replica { source: first, transform: reflected_x() });
    let shifted = offset(&mut ir, "outer", second, 1.0);
    let result = evaluate(&ir, &shifted, 0.0, 0.0, SurfaceRequest::Second);
    assert_eq!(result.point.get(), Point3::new(3.0, 0.0, 0.0));
    assert_eq!(result.first.unwrap()[0].get(), Vector3::new(0.0, 3.0, 0.0));
    assert_eq!(result.second.unwrap()[0].get(), Vector3::new(-3.0, 0.0, 0.0));
    let index = ModelIndex::build(&ir, StandardIndex);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx.charge_work_limit(1, "original Replica refusal").unwrap_err();
    for request in [SurfaceRequest::First, SurfaceRequest::Second] {
        assert!(matches!(model_jet(EvaluationAdmission::Decode(&ctx), &index, &shifted, f64::NAN, f64::NAN, request),
            Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
