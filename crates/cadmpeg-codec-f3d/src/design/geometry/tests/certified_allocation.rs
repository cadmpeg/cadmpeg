// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn zero_collection_context() -> (DecodeArena, DecodePolicy) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    (arena, policy)
}

fn triangle() -> [Point2; 3] {
    [
        Point2::new(0.0, 0.0),
        Point2::new(3.0, 0.0),
        Point2::new(0.0, 3.0),
    ]
}

#[test]
fn certified_polygon_tube_refuses_limit() {
    let (arena, policy) = zero_collection_context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::CertifiedProfileLoop::from_vertices(&triangle(), Some(&ctx)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d certified polygon tube"
    ));
}

#[test]
fn certified_arc_tubes_refuse_limit() {
    let (arena, policy) = zero_collection_context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::certified_arc_tubes(Point2::new(0.0, 0.0),
            positive_radius(1.0), 0.0, std::f64::consts::PI, 0.25, Some(&ctx)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d certified arc tubes"
    ));
}

#[test]
fn certified_analytic_tube_refuses_limit() {
    let (arena, policy) = zero_collection_context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let segments = [super::super::ProfileBoundarySegment::Line {
        start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0),
    }];
    assert!(matches!(
        super::super::certified_analytic_loop(&segments, Some(&ctx)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d certified analytic tube"
    ));
}

#[test]
fn certified_circle_refuses_arc_tube_limit() {
    let (arena, policy) = zero_collection_context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::certified_circle(Point2::new(100_000_000.0, 0.0),
            positive_radius(1.0), Some(&ctx)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d certified arc tubes"
    ));
}

#[test]
fn certified_boundary_borrow_avoids_clone_limit() {
    let loop_ = super::super::CertifiedProfileLoop::from_vertices(&triangle(), None)
        .unwrap().unwrap();
    let boundary = super::super::ProfileBoundary::CertifiedLoop(loop_);
    let (arena, policy) = zero_collection_context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        boundary.certified_loop(Some(&ctx)).unwrap(),
        Some(std::borrow::Cow::Borrowed(_))
    ));
}

#[test]
fn certified_boundary_containment_propagates_tube_limit() {
    let outer = super::super::ProfileBoundary::Polygon(triangle().to_vec());
    let inner_loop = super::super::CertifiedProfileLoop::from_vertices(&[
        Point2::new(0.25, 0.25),
        Point2::new(0.5, 0.25),
        Point2::new(0.25, 0.5),
    ], None).unwrap().unwrap();
    let inner = super::super::ProfileBoundary::CertifiedLoop(inner_loop);
    let (arena, policy) = zero_collection_context();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        outer.strictly_contains(&inner, Some(&ctx)),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d certified polygon tube"
    ));
}
