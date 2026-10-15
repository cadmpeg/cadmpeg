// SPDX-License-Identifier: Apache-2.0

use super::super::ArrangementBoundary;
use super::*;
use cadmpeg_core::CodecError;

const ARRANGEMENT_FACE_TEST_TOLERANCE: f64 = 1.0e-7;

fn context_with_collection_limit(limit: u64) -> (DecodeArena, DecodePolicy) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    (arena, policy)
}

#[test]
fn node_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut nodes = Vec::new();
    assert!(matches!(
        super::super::arrangement_node(&mut nodes, Point2::new(0.0, 0.0),
            0.0, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement node"
    ));
}

#[test]
fn pending_node_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::arrangement_has_alternate_path(&[], 0, 0, 0, 1, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement pending nodes"
    ));
}

#[test]
fn circle_angle_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::arrangement_circle_angles(
            &[Point2::new(1.0, 0.0)], Point2::new(0.0, 0.0),
            positive_radius(1.0), 0.0, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement circle angle"
    ));
}

#[test]
fn boundary_entity_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let entity = SketchEntityId::mint("synthetic:test:id#edge").unwrap();
    let boundary = ArrangementBoundary {
        entity: &entity,
        parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
        reversed: false,
    };
    assert!(matches!(
        super::super::copy_arrangement_boundary(&ctx, &boundary),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement boundary entity id"
    ));
}

#[test]
fn selected_boundary_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let entity = SketchEntityId::mint("synthetic:test:id#edge").unwrap();
    let boundary = ArrangementBoundary {
        entity: &entity,
        parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
        reversed: false,
    };
    assert!(matches!(
        super::super::copy_arrangement_boundary_run(&ctx, &[boundary]),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement selected boundary"
    ));
}

#[test]
fn arrangement_admitted_route_keeps_two_faces() {
    let (sketch, entities, _, _) = super::coincident_circle_arc_arrangement();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let budget = super::local_arrangement_budget();
    let faces = super::sketch_arrangement_faces(
        &sketch,
        &entities,
        ARRANGEMENT_FACE_TEST_TOLERANCE,
        &budget,
        &ctx,
    )
    .unwrap()
    .unwrap();
    assert_eq!(faces.len(), 2);
}

#[test]
fn arrangement_candidates_borrow_original_boundary_ids() {
    let (sketch, entities, _, _) = super::coincident_circle_arc_arrangement();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let budget = super::local_arrangement_budget();
        let (faces, storage) = ctx
            .with_scoped_storage("test arrangement candidate storage", || {
                super::sketch_arrangement_faces(
                    &sketch,
                    &entities,
                    ARRANGEMENT_FACE_TEST_TOLERANCE,
                    &budget,
                    ctx,
                )
            })
            .unwrap();
        let faces = faces.unwrap();
        assert_eq!(faces.len(), 2);
        for face in &faces {
            assert!(!face.boundary.is_empty());
            for boundary in &face.boundary {
                assert!(
                    entities
                        .iter()
                        .any(|entity| std::ptr::eq(entity.id(), boundary.entity))
                        || sketch
                            .profiles
                            .iter()
                            .flatten()
                            .any(|use_| { std::ptr::eq(&raw const use_.entity, boundary.entity) })
                );
            }
        }
        drop((faces, storage));
        let remaining = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test arrangement storage release",
            )
            .unwrap();
        drop(remaining);
        assert!(ctx.resource_refusal().is_none());
    });
}
