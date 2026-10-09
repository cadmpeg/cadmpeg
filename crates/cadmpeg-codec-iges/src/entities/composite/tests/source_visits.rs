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
