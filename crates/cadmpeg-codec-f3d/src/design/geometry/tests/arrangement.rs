// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::design::geometry as owner;

#[test]
fn arrangement_visit_marks_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = owner::arrangement_has_alternate_path(&[], 0, 0, 0, 2, &ctx)
        .expect_err("two visit marks exceed one admitted item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 2
    ));
}

#[test]
fn arrangement_outgoing_refuses_collection_limit() {
    let limit = arrangement_refusal_at_operation("f3d_arrangement_outgoing");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.used + limit.additional, limit.limit + 1);
    assert_eq!(limit.additional, 2);
}

#[test]
fn arrangement_outgoing_entries_refuse_collection_limit() {
    let (sketch, entities, _, _) = coincident_circle_arc_arrangement();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "f3d arrangement outgoing entry",
        0,
        |ctx| {
            let budget = local_arrangement_budget();
            sketch_arrangement_faces(&sketch, &entities, EPS_ARRANGEMENT_SESSION, &budget, ctx)
                .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "f3d arrangement outgoing entry")
    );
}

#[test]
fn arrangement_pending_boundaries_refuse_materialized_limit() {
    let (mut sketch, mut entities, _, _) = coincident_circle_arc_arrangement();
    for region in 0..32 {
        let x = 4.0 + f64::from(region) * 3.0;
        let points = [
            Point2::new(x, 0.0),
            Point2::new(x + 1.0, 0.0),
            Point2::new(x, 1.0),
        ];
        let mut boundary = Vec::new();
        for edge in 0..3 {
            let id =
                SketchEntityId::mint(format!("synthetic:test:outgoing#{region}-{edge}")).unwrap();
            entities.push(SketchEntity::new(
                id.clone(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[edge],
                    end: points[(edge + 1) % 3],
                })
                .unwrap(),
            ));
            boundary.push(SketchEntityUse {
                entity: id,
                reversed: false,
            });
        }
        sketch.profiles.try_push(boundary).unwrap();
    }
    crate::test_support::with_decode_context(|ctx| {
        let budget = local_arrangement_budget();
        assert_eq!(
            sketch_arrangement_faces(&sketch, &entities, EPS_ARRANGEMENT_SESSION, &budget, ctx)
                .unwrap()
                .unwrap()
                .len(),
            34
        );
    });
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "f3d arrangement pending boundary",
        0,
        |ctx| {
            let budget = local_arrangement_budget();
            sketch_arrangement_faces(&sketch, &entities, EPS_ARRANGEMENT_SESSION, &budget, ctx)
                .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "f3d arrangement pending boundary")
    );
}

#[test]
fn arrangement_edge_visits_refuse_collection_limit() {
    let limit = arrangement_refusal_at_operation("f3d arrangement edge visits");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.used + limit.additional, limit.limit + 1);
    assert_eq!(limit.additional, 3);
}

#[test]
fn arrangement_cycle_work_rejects_unrepresentable_traversal_bound() {
    let edge_count = 1usize << (usize::BITS / 2);
    let error = crate::test_support::with_decode_context(|ctx| {
        owner::arrangement_cycle_work(edge_count, 1, ctx)
    })
    .expect_err("the squared work count cannot be represented");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "F3D arrangement cycle work")
    );
}

#[test]
fn arrangement_cycle_work_admits_cubic_ring_bound() {
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            owner::arrangement_cycle_work(1_000, 1_000, ctx)
        })
        .unwrap(),
        1_002_001_000
    );
}

#[test]
fn arrangement_ring_refuses_unadmitted_cubic_scan() {
    let entity = SketchEntityId::mint("synthetic:test:id#ring").unwrap();
    let boundary = owner::ArrangementBoundary {
        entity: &entity,
        parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
        reversed: false,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 250_000;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut edges: Vec<_> = (0..500)
            .map(|index| owner::SketchArrangementEdge {
                nodes: [index, (index + 1) % 500],
                boundary,
                polyline: Vec::new(),
                _polyline_storage: ctx
                    .reserve_scoped(0, "test arrangement empty polyline storage")
                    .unwrap(),
            })
            .collect();
        let budget = ctx.work_budget(1_000_000_000);
        let error =
            owner::arrangement_retain_cycle_edges(&mut edges, 500, &budget, ctx).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && ctx.resource_refusal() == Some(limit))
        );
        assert_eq!(edges.len(), 500);
    });
}
