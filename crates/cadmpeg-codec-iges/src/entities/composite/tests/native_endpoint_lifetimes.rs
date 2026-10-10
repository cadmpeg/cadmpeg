// SPDX-License-Identifier: Apache-2.0

use super::super::{project_native_composite, CompositeIndex};
use super::*;
use crate::entities::geometry::SourceSequences;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::CompositeCurveSegment;
use std::mem::{align_of, size_of};

const CHILDREN: usize = 64;
const OUTPUT_SLOTS: usize = 64;

fn fixture() -> (CadIr, CompositeIndex, CurveId) {
    let child = CurveId::mint("test:model:curve#child").unwrap();
    let start = VertexId::mint("test:model:vertex#start").unwrap();
    let end = VertexId::mint("test:model:vertex#end").unwrap();
    let mut ir = CadIr::empty();
    ir.model.points.reserve_exact(OUTPUT_SLOTS);
    for index in 0..OUTPUT_SLOTS {
        let point =
            FinitePoint3::new(Point3::new(if index == 0 { 0.0 } else { 1.0 }, 0.0, 0.0)).unwrap();
        ir.model.points.push(Point::new(
            PointId::mint(format!("test:model:point#{index}")).unwrap(),
            point,
            None,
        ));
    }
    ir.model.vertices.extend([
        Vertex {
            id: start.clone(),
            point: ir.model.points[0].id.clone(),
            tolerance: None,
        },
        Vertex {
            id: end.clone(),
            point: ir.model.points[1].id.clone(),
            tolerance: None,
        },
    ]);
    ir.model.curves.push(Curve {
        id: child.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#child").unwrap(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(child.clone()), Some([0.0, 1.0]))
            .unwrap(),
        start,
        end,
        tolerance: None,
    });
    let index = crate::test_support::with_service_context(&[], |ctx| {
        CompositeIndex::from_ir(&ir, ctx).unwrap()
    });
    (ir, index, child)
}

fn prefix_bytes(child: &CurveId) -> u64 {
    let point_ids = "iges:model:point#D5-start".len() + "iges:model:point#D5-end".len();
    let vertex_ids = "iges:model:vertex#D5-start".len() + "iges:model:vertex#D5-end".len();
    let point_node = 11 * (size_of::<PointId>() + size_of::<u32>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<PointId>()
            .max(align_of::<u32>())
            .max(align_of::<usize>());
    // Surviving segments and child IDs, generated point IDs and their
    // sequence keys, vertex IDs, curve/edge IDs, and one point-sequence node.
    // The copied endpoint positions have no later reader.
    u64_from_index(
        CHILDREN * (size_of::<CompositeCurveSegment>() + child.as_str().len())
            + 2 * point_ids
            + vertex_ids
            + "iges:model:curve#D5".len()
            + "iges:model:edge#D5".len()
            + point_node,
    )
}

fn phase_boundary(exact_growth: bool) {
    let (mut ir, mut index, child) = fixture();
    let before = ir.clone();
    let children = vec![&child; CHILDREN];
    let entry = crate::test_support::directory_target(5, 102);
    let growth = u64_from_index(OUTPUT_SLOTS * size_of::<Point>());
    let prefix = prefix_bytes(&child);
    let cap = prefix + growth - u64::from(!exact_growth);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut index_storage = ctx.reserve_scoped(0, "test index growth").unwrap();
    let mut output = ctx
        .reserve_scoped(0, "test native composite output")
        .unwrap();
    let error = output
        .with_storage(|| {
            project_native_composite(
                &mut ir,
                (&mut index, &mut index_storage),
                &entry,
                &children,
                0.0,
                &ctx,
                &mut sequences,
            )
        })
        .unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected point slot refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.operation, "iges composite native point slots");
    assert_eq!(
        (first.limit, first.used, first.additional),
        (cap, prefix + if exact_growth { growth } else { 0 }, growth)
    );
    assert_eq!(ir, before);
    for _ in 0..64 {
        for source in [children.as_slice(), &[]] {
            assert!(
                matches!(project_native_composite(&mut ir, (&mut index, &mut index_storage),
                &entry, source, 0.0, &ctx, &mut sequences), Err(CodecError::ResourceLimit(last)) if last == first)
            );
            assert_eq!(ir, before);
        }
    }
    drop(ir);
    drop(index);
    drop(sequences);
    drop(index_storage);
    drop(output);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn native_composite_endpoints_end_before_one_short_point_growth() {
    phase_boundary(false);
}

#[test]
fn native_composite_exact_point_growth_reaches_buffer_overlap() {
    phase_boundary(true);
}

#[test]
fn native_composite_endpoint_release_preserves_segments_and_identity() {
    let (mut ir, mut index, child) = fixture();
    let before = ir.model.clone();
    let children = vec![&child; CHILDREN];
    let entry = crate::test_support::directory_target(5, 102);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut index_storage = ctx.reserve_scoped(0, "test index growth").unwrap();
    let mut output = ctx
        .reserve_scoped(0, "test native composite output")
        .unwrap();
    let edge = output
        .with_storage(|| {
            project_native_composite(
                &mut ir,
                (&mut index, &mut index_storage),
                &entry,
                &children,
                0.0,
                &ctx,
                &mut sequences,
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(edge.as_str(), "iges:model:edge#D5");
    drop(edge);
    assert_eq!(&ir.model.points[..OUTPUT_SLOTS], before.points.as_slice());
    assert_eq!(&ir.model.vertices[..2], before.vertices.as_slice());
    assert_eq!(&ir.model.curves[..1], before.curves.as_slice());
    assert_eq!(&ir.model.edges[..1], before.edges.as_slice());
    assert_eq!(ir.model.points.len(), OUTPUT_SLOTS + 2);
    assert_eq!(ir.model.vertices.len(), 4);
    assert_eq!(ir.model.curves.len(), 2);
    assert_eq!(ir.model.edges.len(), 2);
    assert_eq!(ir.model.curves[1].id.as_str(), "iges:model:curve#D5");
    let CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments,
        self_intersect,
    }) = &ir.model.curves[1].geometry
    else {
        panic!("expected native composite");
    };
    assert_eq!(segments.len(), CHILDREN);
    assert_eq!(*self_intersect, None);
    for segment in segments {
        assert_eq!(segment.curve, child);
        assert!(segment.same_sense);
        assert_eq!(
            segment.transition,
            cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous
        );
    }
    let edge = &ir.model.edges[1];
    assert_eq!(edge.curve().unwrap().as_str(), "iges:model:curve#D5");
    assert_eq!(edge.start.as_str(), "iges:model:vertex#D5-start");
    assert_eq!(edge.end.as_str(), "iges:model:vertex#D5-end");
    assert_eq!(edge.param_range(), None);
    assert_eq!(
        ir.model.points[OUTPUT_SLOTS].position().get(),
        Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        ir.model.points[OUTPUT_SLOTS + 1].position().get(),
        Point3::new(1.0, 0.0, 0.0)
    );
    drop(ir);
    drop(index);
    drop(sequences);
    drop(index_storage);
    drop(output);
    let released = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test released scratch",
        )
        .unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

fn native_segment_source_boundary(count: usize, visited: usize, before_copy: bool, complete: bool) {
    let (mut ir, mut index, child) = fixture();
    // A NURBS carrier uses the indexed edge range without the separate line
    // endpoint evaluation. Its two stored points still define the endpoints.
    ir.model.curves[0].geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(test_nurbs(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
    )));
    let before = ir.clone();
    let children = vec![&child; count];
    let entry = crate::test_support::directory_target(5, 102);
    let child_bytes = u64_from_index(child.as_str().len());
    let vertex_bytes = u64_from_index(
        index.edges[&child][0].start.as_str().len() + index.edges[&child][0].end.as_str().len(),
    );
    // Each endpoint visits its child and edge candidate, looks up the child
    // in each one-key tree, copies both selected vertex IDs, then reads both
    // endpoints from the two-key vertex tree. No tail/end source is executed.
    let endpoint_work = u64_from_index(count) * (2 + 2 * child_bytes + 3 * vertex_bytes);
    let completed_segments = if complete { count } else { visited };
    let work = endpoint_work
        + u64_from_index(completed_segments) * (1 + child_bytes)
        + u64::from(before_copy && !complete);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut index_storage = ctx.reserve_scoped(0, "test index growth").unwrap();
    let mut output = ctx
        .reserve_scoped(0, "test native composite output")
        .unwrap();
    let error = output
        .with_storage(|| {
            project_native_composite(
                &mut ir,
                (&mut index, &mut index_storage),
                &entry,
                &children,
                0.0,
                &ctx,
                &mut sequences,
            )
        })
        .unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected native segment work refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!((first.limit, first.used), (work, work));
    if complete {
        // The whole fixed-step source and all child copies finish. The next
        // operation formats the first point ID; its emitted fragment is not
        // part of the source traversal's exact work total.
        assert_eq!(first.operation, "iges generated identity");
        assert!(first.additional > 0);
        assert!(first.additional <= u64_from_index("iges:model:point#D5-start".len()));
    } else if before_copy {
        assert_eq!(first.operation, "iges composite native segment curve ids");
        assert_eq!(first.additional, child_bytes);
    } else {
        assert_eq!(first.operation, "iges composite native segment traversal");
        assert_eq!(first.additional, 1);
    }
    assert_eq!(ir, before);
    for _ in 0..64 {
        for source in [children.as_slice(), &[]] {
            assert!(
                matches!(project_native_composite(&mut ir, (&mut index, &mut index_storage),
                &entry, source, 0.0, &ctx, &mut sequences), Err(CodecError::ResourceLimit(last)) if last == first)
            );
            assert_eq!(ir, before);
        }
    }
    drop(ir);
    drop(index);
    drop(sequences);
    drop(index_storage);
    drop(output);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn native_composite_segments_refuse_first_and_last_source_visits() {
    for count in [1, CHILDREN] {
        for visited in [0, count - 1] {
            native_segment_source_boundary(count, visited, false, false);
        }
    }
}

#[test]
fn native_composite_segments_refuse_first_and_last_identity_copies() {
    for count in [1, CHILDREN] {
        for visited in [0, count - 1] {
            native_segment_source_boundary(count, visited, true, false);
        }
    }
}

#[test]
fn native_composite_complete_segments_reach_output_without_an_end_probe() {
    for count in [1, CHILDREN] {
        native_segment_source_boundary(count, 0, false, true);
    }
    let (mut ir, mut index, _) = fixture();
    let before = ir.clone();
    let entry = crate::test_support::directory_target(5, 102);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let mut index_storage = ctx.reserve_scoped(0, "test index growth").unwrap();
    assert!(project_native_composite(
        &mut ir,
        (&mut index, &mut index_storage),
        &entry,
        &[],
        0.0,
        &ctx,
        &mut sequences
    )
    .unwrap()
    .is_none());
    assert_eq!(ir, before);
    drop(sequences);
    drop(index_storage);
    ctx.finish_session().unwrap();
}
