// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::{select_composite_edge, CompositeEdge, CompositeIndex};

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
            Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
        ).unwrap(),
    )
}

#[test]
fn composite_candidate_source_refuses_one_visit_and_preserves_empty_replay() {
    let candidates = candidates(None);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match select_composite_edge(&ctx, &CadIr::empty(), None, &line(), &candidates, 0.0) {
        Err(CodecError::ResourceLimit(first)) => first,
        Err(error) => panic!("unexpected candidate error: {error:?}"),
        Ok(_) => panic!("expected candidate source refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges composite edge candidates");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [&candidates[..], &[]] {
        assert!(matches!(select_composite_edge(&ctx, &CadIr::empty(), None, &line(), replay, 0.0),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_candidate_lookup_refuses_after_one_visit_without_admitting_the_tail() {
    let candidates = candidates(Some([0.0, 1.0]));
    let mut index = CompositeIndex::default();
    index.vertex_points.insert(candidates[0].start.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match select_composite_edge(&ctx, &CadIr::empty(), Some(&index), &line(), &candidates, 0.0) {
        Err(CodecError::ResourceLimit(first)) => first,
        Err(error) => panic!("unexpected candidate lookup error: {error:?}"),
        Ok(_) => panic!("expected first vertex lookup refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges composite vertex lookup");
    // A one-entry tree admits one complete identity comparison.
    assert_eq!((first.limit, first.used, first.additional),
        (1, 1, u64::try_from(candidates[0].start.as_str().len()).unwrap()));
    assert!(matches!(select_composite_edge(&ctx, &CadIr::empty(), Some(&index), &line(), &[], 0.0),
        Err(CodecError::ResourceLimit(last)) if last == first));
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
        assert!(select_composite_edge(&ctx, &CadIr::empty(), None, &line(), source, 0.0).unwrap().is_none());
        ctx.finish_session().unwrap();
    }
}

fn unindexed_edges(count: usize, foreign_carrier: bool) -> (CadIr, CurveId, u64, u64) {
    let id = CurveId::mint("test:model:curve#target").unwrap();
    let other = CurveId::mint("test:model:curve#different").unwrap();
    let prelude = 1 + 2 * u64::try_from(id.as_str().len()).unwrap();
    let comparison = if foreign_carrier {
        u64::try_from(other.as_str().len() + id.as_str().len()).unwrap()
    } else { 0 };
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve { id: id.clone(), geometry: CurveGeometry::Solved(line()), source_object: None });
    for index in 0..count {
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("test:model:edge#{index}")).unwrap(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(foreign_carrier.then(|| other.clone()), None).unwrap(),
            start: VertexId::mint("test:model:vertex#start").unwrap(),
            end: VertexId::mint("test:model:vertex#end").unwrap(), tolerance: None,
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
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &edge_scan_policy(cap)).unwrap();
                let first = match super::super::bounded_edge_for_curve(&ir, &id, 0.0, None, &ctx) {
                    Err(CodecError::ResourceLimit(first)) => first,
                    _ => panic!("expected actual unindexed edge source refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, "iges composite scanned edge traversal");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                for _ in 0..64 {
                    for source in [&ir, &CadIr::empty()] {
                        assert!(matches!(super::super::bounded_edge_for_curve(source, &id, 0.0, None, &ctx),
                            Err(CodecError::ResourceLimit(last)) if last == first));
                    }
                }
                assert_eq!(ir, before);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
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
        let first = match super::super::bounded_edge_for_curve(&ir, &id, 0.0, None, &ctx) {
            Err(CodecError::ResourceLimit(first)) => first,
            _ => panic!("expected input-sized carrier equality refusal"),
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges composite scanned edge carrier");
        assert_eq!((first.limit, first.used, first.additional), (cap, cap, additional));
        for _ in 0..64 {
            for source in [&ir, &CadIr::empty()] {
                assert!(matches!(super::super::bounded_edge_for_curve(source, &id, 0.0, None, &ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
        assert_eq!(ir, before);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
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
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &edge_scan_policy(cap)).unwrap();
            assert!(super::super::bounded_edge_for_curve(&ir, &id, 0.0, None, &ctx).unwrap().is_none());
            assert_eq!(ir, before);
            ctx.finish_session().unwrap();
        }
    }
}

fn internal_knot_boundary(visited: usize, accepts_source: bool) {
    let mut curve = test_nurbs(1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None);
    let before = serde_json::to_value(&curve).unwrap();
    // On [0,0,1,1], lower/upper partition paths visit 3/2 slots
    // for zero and 2/2 slots for one. The two homogeneous controls
    // then need two visits, followed by four copied knots.
    let boundary_visits = 3 + 2 + 2 + 2;
    let prelude = boundary_visits + u64::try_from(curve.pole_count() + curve.knots().len()).unwrap();
    let cap = prelude + u64::try_from(visited).unwrap();
    let arena = DecodeArena::new();
    let mut policy = edge_scan_policy(cap);
    policy.limits.max_collection_items = u64::try_from(curve.pole_count() + curve.knots().len()).unwrap();
    policy.limits.max_materialized_bytes = u64::try_from(
        curve.pole_count() * std::mem::size_of::<[f64; 4]>()
            + curve.knots().len() * std::mem::size_of::<f64>(),
    ).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = match elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], 2, None) {
        Err(error) => error,
        Ok(()) => panic!("expected source or next phase refusal"),
    };
    let first = match error.non_resource() {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected the original resource refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, if accepts_source { "iges composite elevation spans" }
        else { "iges composite internal knot traversal" });
    assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
    for _ in 0..64 {
        for target in [2, 1] {
            let error = match elevate_nurbs_to_degree(&ctx, &mut curve, [0.0, 1.0], target, None) {
                Err(error) => error,
                Ok(()) => panic!("expected sticky refusal before same-degree recovery"),
            };
            assert!(matches!(error.non_resource(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
    assert_eq!(serde_json::to_value(&curve).unwrap(), before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_internal_knot_source_refuses_first_and_last_actual_visits() {
    for visited in [0, 3] { internal_knot_boundary(visited, false); }
}

#[test]
fn composite_internal_knot_phase_accepts_exact_prelude_and_source_work() {
    // Four actual knot visits complete without allocating an internal value;
    // the exact phase limit next refuses the first elevated-span visit.
    internal_knot_boundary(4, true);
}
