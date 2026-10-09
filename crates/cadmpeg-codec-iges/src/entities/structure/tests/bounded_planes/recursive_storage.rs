// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::CodecError;

fn composite_with_line_child() -> (CadIr, SolvedCurveGeometry) {
    let child = CurveId::mint("iges:model:curve#D1").unwrap();
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: child.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        )),
        source_object: None,
    });
    let geometry = SolvedCurveGeometry::Composite {
        segments: vec![CompositeCurveSegment {
            curve: child,
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }].try_into().unwrap(),
        self_intersect: Some(false),
    };
    (ir, geometry)
}

#[test]
fn bounded_plane_child_depth_refusal_destroys_path_before_frame_storage() {
    let (ir, geometry) = composite_with_line_child();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    for cap in [1, 2] {
        let mut policy = DecodePolicy::service();
        // The composite enters first; its line child needs the second frame.
        policy.limits.max_recursion_depth = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut active = BTreeSet::new();
        let result = super::super::super::bounded_plane_curve_is_simple(
            &geometry, super::super::super::PlaneBoundarySimplicity {
                index: &index,
                plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                resolution: 0.001,
                transform: Transform::identity(),
                ctx: &ctx,
            }, false, None, &mut active,
        );
        assert!(active.is_empty());
        if cap == 1 {
            let Err(CodecError::ResourceLimit(first)) = result else {
                panic!("expected child depth refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
            assert_eq!(first.operation, "iges plane boundary simplicity");
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            assert!(!result.unwrap());
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn bounded_plane_removal_refusal_destroys_path_before_frame_storage() {
    let (ir, geometry) = composite_with_line_child();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "iges plane boundary active removal", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut active = BTreeSet::new();
            let result = super::super::super::bounded_plane_curve_is_simple(
                &geometry, super::super::super::PlaneBoundarySimplicity {
                    index: &index,
                    plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                    resolution: 0.001,
                    transform: Transform::identity(),
                    ctx: &ctx,
                }, false, None, &mut active,
            );
            assert!(active.is_empty());
            let Err(CodecError::ResourceLimit(first)) = &result else {
                panic!("expected removal refusal")
            };
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first));
            result
        },
    );
}

fn node_bytes() -> u64 {
    u64::try_from(11 * std::mem::size_of::<CurveId>() + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<CurveId>().max(std::mem::align_of::<usize>())).unwrap()
}

fn assert_nonsimple_child_root_destruction(unsolved: bool) {
    let (mut ir, geometry) = composite_with_line_child();
    if unsolved {
        ir.model.curves[0].geometry = CurveGeometry::Procedural {
            construction: cadmpeg_ir::ids::ProceduralCurveId::mint("iges:model:procedural-curve#D1").unwrap(),
            cache: None,
        };
    }
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let key = u64::try_from(ir.model.curves[0].id.as_str().len()).unwrap();
    let node = node_bytes();
    for cap in [key + node - 1, key + node] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 1;
        policy.limits.max_recursion_depth = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut active = BTreeSet::new();
        let result = super::super::super::bounded_plane_curve_is_simple(&geometry,
            super::super::super::PlaneBoundarySimplicity {
                index: &index, plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                resolution: 0.001, transform: Transform::identity(), ctx: &ctx,
            }, false, None, &mut active);
        assert!(active.is_empty());
        if cap == key + node {
            assert!(!result.unwrap());
            let released = ctx.reserve_scoped(cap, "test destroyed nonsimple boundary root").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(first)) = result else { panic!("expected boundary active node refusal") };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges plane boundary active curve");
            assert_eq!((first.limit, first.used, first.additional), (cap, key, node));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}

#[test]
fn bounded_plane_unsolved_child_destroys_empty_root_before_frame_refund() {
    assert_nonsimple_child_root_destruction(true);
}

#[test]
fn bounded_plane_nonsimple_child_destroys_empty_root_before_frame_refund() {
    assert_nonsimple_child_root_destruction(false);
}

#[test]
fn bounded_plane_nonsimple_child_preserves_seeded_ancestor_storage() {
    let (ir, geometry) = composite_with_line_child();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let seed = CurveId::mint("iges:model:curve#seed").unwrap();
    let seed_bytes = u64::try_from(seed.as_str().len()).unwrap();
    let child_bytes = u64::try_from(ir.model.curves[0].id.as_str().len()).unwrap();
    let node = node_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = node + seed_bytes + child_bytes;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test seeded boundary ancestor storage").unwrap();
    let mut active = BTreeSet::new();
    storage.with_storage(|| {
        let key = seed.try_clone_for_decode(&ctx, "test seeded boundary ancestor key")?;
        ctx.insert_btree_set(&mut active, key, "test seeded boundary ancestor node")
    }).unwrap();
    assert!(!super::super::super::bounded_plane_curve_is_simple(&geometry,
        super::super::super::PlaneBoundarySimplicity {
            index: &index, plane: (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            resolution: 0.001, transform: Transform::identity(), ctx: &ctx,
        }, false, None, &mut active).unwrap());
    assert_eq!(active, BTreeSet::from([seed]));
    let child_released = ctx.reserve_scoped(child_bytes, "test removed nonsimple child backing").unwrap();
    drop(child_released);
    drop(active);
    drop(storage);
    let released = ctx.reserve_scoped(node + seed_bytes + child_bytes, "test destroyed seeded boundary path").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}
