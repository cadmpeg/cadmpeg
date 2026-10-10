// SPDX-License-Identifier: Apache-2.0

use super::super::{select_composite_edge, CompositeEdge, CompositeIndex};
use super::*;
use cadmpeg_ir::geometry::nurbs::NurbsPoles3;

fn candidates(range: Option<[f64; 2]>) -> [CompositeEdge; 3] {
    std::array::from_fn(|_| CompositeEdge {
        start: VertexId::mint("test:model:vertex#start").unwrap(),
        end: VertexId::mint("test:model:vertex#end").unwrap(),
        param_range: range,
    })
}

fn line() -> SolvedCurveGeometry {
    SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    )
}

#[test]
fn composite_candidate_source_refuses_one_visit_and_preserves_empty_replay() {
    let candidates = candidates(None);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match select_composite_edge(&ctx, &CadIr::empty(), None, &line(), &candidates, 0.0)
    {
        Err(CodecError::ResourceLimit(first)) => first,
        Err(error) => panic!("unexpected candidate error: {error:?}"),
        Ok(_) => panic!("expected candidate source refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges composite edge candidates");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [&candidates[..], &[]] {
        assert!(
            matches!(select_composite_edge(&ctx, &CadIr::empty(), None, &line(), replay, 0.0),
            Err(CodecError::ResourceLimit(last)) if last == first)
        );
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_candidate_lookup_refuses_after_one_visit_without_admitting_the_tail() {
    let candidates = candidates(Some([0.0, 1.0]));
    let mut index = CompositeIndex::default();
    index.vertex_points.insert(
        candidates[0].start.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match select_composite_edge(
        &ctx,
        &CadIr::empty(),
        Some(&index),
        &line(),
        &candidates,
        0.0,
    ) {
        Err(CodecError::ResourceLimit(first)) => first,
        Err(error) => panic!("unexpected candidate lookup error: {error:?}"),
        Ok(_) => panic!("expected first vertex lookup refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges composite vertex lookup");
    // A one-entry tree admits one complete identity comparison.
    assert_eq!(
        (first.limit, first.used, first.additional),
        (
            1,
            1,
            u64::try_from(candidates[0].start.as_str().len()).unwrap()
        )
    );
    assert!(
        matches!(select_composite_edge(&ctx, &CadIr::empty(), Some(&index), &line(), &[], 0.0),
        Err(CodecError::ResourceLimit(last)) if last == first)
    );
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_unbounded_candidates_accept_exact_visits_without_an_end_probe() {
    let candidates = candidates(None);
    for source in [&candidates[..], &[]] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(source.len()).unwrap();
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            select_composite_edge(&ctx, &CadIr::empty(), None, &line(), source, 0.0)
                .unwrap()
                .is_none()
        );
        ctx.finish_session().unwrap();
    }
}

fn unindexed_edges(count: usize, foreign_carrier: bool) -> (CadIr, CurveId, u64, u64) {
    let id = CurveId::mint("test:model:curve#target").unwrap();
    let other = CurveId::mint("test:model:curve#different").unwrap();
    let prelude = 1 + 2 * u64::try_from(id.as_str().len()).unwrap();
    let comparison = if foreign_carrier {
        u64::try_from(other.as_str().len() + id.as_str().len()).unwrap()
    } else {
        0
    };
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(line()),
        source_object: None,
    });
    for index in 0..count {
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("test:model:edge#{index}")).unwrap(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                foreign_carrier.then(|| other.clone()),
                None,
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#start").unwrap(),
            end: VertexId::mint("test:model:vertex#end").unwrap(),
            tolerance: None,
        });
    }
    (ir, id, prelude, comparison)
}

fn edge_scan_policy(work: u64) -> DecodePolicy {
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
fn composite_unindexed_edge_source_refuses_first_and_last_actual_visits() {
    for foreign_carrier in [false, true] {
        for count in [1, 64] {
            let (ir, id, prelude, comparison) = unindexed_edges(count, foreign_carrier);
            let before = ir.clone();
            // Curve lookup visits the first curve and compares both identities.
            // Each free edge then needs one visit; each foreign carrier also
            // compares both carrier identities without constructing a candidate.
            for visited in [0, count - 1] {
                let cap = prelude + u64::try_from(visited).unwrap() * (1 + comparison);
                let arena = DecodeArena::new();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &edge_scan_policy(cap)).unwrap();
                let Err(CodecError::ResourceLimit(first)) =
                    super::super::bounded_edge_for_curve(&ir, &id, 0.0, None, &ctx)
                else {
                    panic!("expected actual unindexed edge source refusal")
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "iges composite scanned edge traversal");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                for _ in 0..64 {
                    for source in [&ir, &CadIr::empty()] {
                        assert!(
                            matches!(super::super::bounded_edge_for_curve(source, &id, 0.0, None, &ctx),
                            Err(CodecError::ResourceLimit(last)) if last == first)
                        );
                    }
                }
                assert_eq!(ir, before);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
                );
            }
        }
    }
}

#[test]
fn composite_unindexed_carrier_comparison_refuses_before_the_source_tail() {
    let (ir, id, prelude, comparison) = unindexed_edges(64, true);
    let before = ir.clone();
    let left = u64::try_from(ir.model.edges[0].curve().unwrap().as_str().len()).unwrap();
    let right = u64::try_from(id.as_str().len()).unwrap();
    assert_eq!(comparison, left + right);
    for (charged, additional) in [(0, left), (left, right)] {
        let cap = prelude + 1 + charged;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &edge_scan_policy(cap)).unwrap();
        let Err(CodecError::ResourceLimit(first)) =
            super::super::bounded_edge_for_curve(&ir, &id, 0.0, None, &ctx)
        else {
            panic!("expected input-sized carrier equality refusal")
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges composite scanned edge carrier");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (cap, cap, additional)
        );
        for _ in 0..64 {
            for source in [&ir, &CadIr::empty()] {
                assert!(
                    matches!(super::super::bounded_edge_for_curve(source, &id, 0.0, None, &ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first)
                );
            }
        }
        assert_eq!(ir, before);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
        );
    }
}

#[test]
fn composite_unindexed_edge_scan_accepts_exact_executed_work_without_backing() {
    for foreign_carrier in [false, true] {
        for count in [0, 1, 64] {
            let (ir, id, prelude, comparison) = unindexed_edges(count, foreign_carrier);
            let before = ir.clone();
            let cap = prelude + u64::try_from(count).unwrap() * (1 + comparison);
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &edge_scan_policy(cap)).unwrap();
            assert!(
                super::super::bounded_edge_for_curve(&ir, &id, 0.0, None, &ctx)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(ir, before);
            ctx.finish_session().unwrap();
        }
    }
}

fn internal_knot_boundary(visited: usize, accepts_source: bool) {
    let mut curve = test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    );
    let before = serde_json::to_value(&curve).unwrap();
    // On [0,0,1,1], lower/upper partition paths visit 3/2 slots
    // for zero and 2/2 slots for one. The two homogeneous controls
    // then need two visits, followed by four copied knots.
    let boundary_visits = 3 + 2 + 2 + 2;
    let prelude =
        boundary_visits + u64::try_from(curve.pole_count() + curve.knots().len()).unwrap();
    let cap = prelude + u64::try_from(visited).unwrap();
    let arena = DecodeArena::new();
    let mut policy = edge_scan_policy(cap);
    policy.limits.max_collection_items =
        u64::try_from(curve.pole_count() + curve.knots().len()).unwrap();
    policy.limits.max_materialized_bytes = u64::try_from(
        curve.pole_count() * std::mem::size_of::<[f64; 4]>()
            + curve.knots().len() * std::mem::size_of::<f64>(),
    )
    .unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(error) = elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], 2, None) else {
        panic!("expected source or next phase refusal")
    };
    let Err(CodecError::ResourceLimit(first)) = error.non_resource() else {
        panic!("expected the original resource refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(
        first.operation,
        if accepts_source {
            "iges composite elevation spans"
        } else {
            "iges composite internal knot traversal"
        }
    );
    assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
    for _ in 0..64 {
        for target in [2, 1] {
            let Err(error) = elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], target, None)
            else {
                panic!("expected sticky refusal before same-degree recovery")
            };
            assert!(
                matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }
    assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_internal_knot_source_refuses_first_and_last_actual_visits() {
    for visited in [0, 3] {
        internal_knot_boundary(visited, false);
    }
}

#[test]
fn composite_internal_knot_phase_accepts_exact_prelude_and_source_work() {
    // Four actual knot visits complete without allocating an internal value;
    // the exact phase limit next refuses the first elevated-span visit.
    internal_knot_boundary(4, true);
}

fn internal_value_curve(rational: bool) -> NurbsCurve {
    test_nurbs(
        2,
        vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
        ],
        rational.then(|| vec![1.0, 2.0, 1.0, 3.0]),
    )
}

fn internal_value_storage_boundary(rational: bool, exact: bool) {
    let mut curve = internal_value_curve(rational);
    let before = serde_json::to_value(&curve).unwrap();
    // Insertion leaves five homogeneous controls and eight knots. The
    // earlier overlap is (4*32 + 7*8) + (5*32 + 8*8) + 4*8 = 440.
    // The first elevated net needs four controls while its three-control
    // source stays live. The consumed four-slot internal-value vector
    // has no reader in this phase and must no longer contribute bytes.
    let scalar = std::mem::size_of::<f64>();
    let homogeneous = std::mem::size_of::<[f64; 4]>();
    let prefix = u64::try_from(5 * homogeneous + 8 * scalar).unwrap();
    let source = u64::try_from(3 * homogeneous).unwrap();
    let additional = u64::try_from(4 * homogeneous).unwrap();
    let peak = prefix + source + additional;
    let cap = peak - u64::from(!exact);
    let earlier_overlap = u64::try_from(9 * homogeneous + 19 * scalar).unwrap();
    assert!(earlier_overlap < cap);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    if exact {
        // Four homogeneous controls, seven copied knots, one internal
        // value, eight inserted knots, five inserted controls, three
        // copied Bezier controls and four elevated controls. Stop at
        // Euclidean allocation before judging later scratch lifetimes.
        policy.limits.max_collection_items = 4 + 7 + 1 + 8 + 5 + 3 + 4;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], 3, None)
        .expect_err("expected the next allocation boundary");
    let Err(CodecError::ResourceLimit(first)) = error.non_resource() else {
        panic!("expected the original materialized-byte refusal")
    };
    if exact {
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "iges composite Euclidean control points");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (
                policy.limits.max_collection_items,
                policy.limits.max_collection_items,
                4
            )
        );
    } else {
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "iges composite Bezier elevated net");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (cap, prefix + source, additional)
        );
    }
    for _ in 0..64 {
        for degree in [3, 2] {
            let error = elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], degree, None)
                .expect_err("expected refusal before the unchanged-degree route");
            assert!(
                matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }
    assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_internal_values_release_before_one_short_elevated_net() {
    for rational in [false, true] {
        internal_value_storage_boundary(rational, false);
    }
}

#[test]
fn composite_internal_values_exact_net_peak_reaches_the_next_lane() {
    for rational in [false, true] {
        internal_value_storage_boundary(rational, true);
    }
}

fn elevated_geometry_and_backing(degree: u32) {
    const EPS_ELEVATED_POINT: f64 = 1.0e-10;
    for rational in [false, true] {
        let mut curve = internal_value_curve(rational);
        let parameters = [0.0, 0.125, 0.5, 0.875, 1.0];
        let expected = parameters.map(|parameter| {
            cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &curve,
                parameter,
            )
            .unwrap()
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut output = ctx.reserve_scoped(0, "test elevated curve output").unwrap();
        output
            .with_storage(|| elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], degree, None))
            .unwrap();
        assert_eq!(curve.degree(), degree);
        let multiplicity = usize::try_from(degree).unwrap() + 1;
        assert_eq!(&curve.knots()[..multiplicity], vec![0.0; multiplicity]);
        assert_eq!(
            &curve.knots()[curve.knots().len() - multiplicity..],
            vec![1.0; multiplicity]
        );
        assert_eq!(
            matches!(curve.pole_rows(), NurbsPoles3::Rational { .. }),
            rational
        );
        for (parameter, expected) in parameters.into_iter().zip(expected) {
            let actual = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &curve,
                parameter,
            )
            .unwrap();
            assert!(actual.distance(expected.get()) <= EPS_ELEVATED_POINT);
        }
        drop(curve);
        drop(output);
        let free = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test elevated backing released",
            )
            .unwrap();
        drop(free);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn composite_internal_value_release_preserves_elevated_geometry_and_backing() {
    elevated_geometry_and_backing(3);
}

fn elevated_net_storage_boundary(rational: bool, exact: bool) {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::scalar::NonZeroReal;
    let mut curve = internal_value_curve(rational);
    let before = serde_json::to_value(&curve).unwrap();
    let scalar = std::mem::size_of::<f64>();
    let refined = 5 * std::mem::size_of::<[f64; 4]>() + 8 * scalar;
    let controls = 4 * std::mem::size_of::<FinitePoint3>();
    let weights = if rational {
        4 * std::mem::size_of::<NonZeroReal>()
    } else {
        0
    };
    // The first piece owns its Euclidean lanes and eight knots. Its
    // homogeneous net has no reader after Euclidean conversion.
    let prefix = u64::try_from(refined + controls + weights + 8 * scalar).unwrap();
    let slots = u64::try_from(4 * std::mem::size_of::<(NurbsCurve, [f64; 2], ())>()).unwrap();
    let cap = prefix + slots - u64::from(!exact);
    let earlier_euclidean_peak =
        u64::try_from(refined + 4 * std::mem::size_of::<[f64; 4]>() + controls + weights).unwrap();
    assert!(earlier_euclidean_peak < cap);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    // Thirty-two items through the first elevated net, four Euclidean
    // controls, optional four weights, four initial and four suffix knots,
    // then one piece. Exact storage stops at the next constructor/source.
    policy.limits.max_collection_items = 32 + 4 + u64::from(rational) * 4 + 4 + 4 + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], 3, None)
        .expect_err("expected piece or next phase refusal");
    let Err(CodecError::ResourceLimit(first)) = error.non_resource() else {
        panic!("expected original piece storage refusal")
    };
    if exact {
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(
            first.operation,
            if rational {
                "IR NURBS paired poles"
            } else {
                "iges composite Bezier source copy"
            }
        );
        let items = policy.limits.max_collection_items;
        assert_eq!(
            (first.limit, first.used, first.additional),
            (items, items, if rational { 1 } else { 3 })
        );
    } else {
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "iges composite elevated span");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (cap, prefix, slots)
        );
    }
    for _ in 0..64 {
        for target in [3, 2] {
            let error = elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], target, None)
                .expect_err("expected original refusal before degree recovery");
            assert!(
                matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }
    assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_elevated_net_releases_before_one_short_piece_storage() {
    for rational in [false, true] {
        elevated_net_storage_boundary(rational, false);
    }
}

#[test]
fn composite_elevated_net_exact_piece_peak_reaches_the_next_phase() {
    for rational in [false, true] {
        elevated_net_storage_boundary(rational, true);
    }
}

#[test]
fn composite_refined_net_release_preserves_high_degree_geometry_and_backing() {
    elevated_geometry_and_backing(64);
}
