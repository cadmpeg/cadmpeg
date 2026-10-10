// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    analytic::LineCurve, CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
    SolvedCurveGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use std::collections::{BTreeMap, BTreeSet};

fn identity_record(sequence: u32) -> ParameterRecord {
    let values = [
        124.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    ParameterRecord::from_test_tokens(
        sequence,
        1..2,
        Vec::new(),
        values.len(),
        values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::real(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    )
}

#[test]
fn transform_child_depth_refusal_destroys_path_before_frame_storage() {
    let parent = super::transform_entry(1, 0);
    let child = super::transform_entry(3, 1);
    let parent_record = identity_record(1);
    let child_record = identity_record(3);
    let entries = BTreeMap::from([(1, &parent), (3, &child)]);
    let records = BTreeMap::from([(1, &parent_record), (3, &child_record)]);
    let precision = crate::global::RealPrecision {
        single_significance: 6,
        double_significance: 15,
    };
    let mut policy = DecodePolicy::service();
    // The first frame inserts D3. Entering its D1 parent requests frame two.
    policy.limits.max_recursion_depth = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut path = BTreeSet::new();
    let error =
        super::super::resolve_transform(3, &entries, &records, 1.0, precision, &mut path, &ctx)
            .unwrap_err();
    let super::super::TransformResolutionError::Resource(CodecError::ResourceLimit(first)) = error
    else {
        panic!("expected child depth refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert_eq!(first.operation, "iges_transform_chain");
    assert!(path.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));

    policy.limits.max_recursion_depth = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut path = BTreeSet::new();
    assert_eq!(
        super::super::resolve_transform(3, &entries, &records, 1.0, precision, &mut path, &ctx,)
            .unwrap(),
        cadmpeg_ir::transform::Transform::identity()
    );
    assert!(path.is_empty());
    ctx.finish_session().unwrap();
}

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
        }]
        .try_into()
        .unwrap(),
        self_intersect: Some(false),
    };
    (ir, geometry)
}

#[test]
fn coplanar_child_depth_refusal_destroys_path_before_frame_storage() {
    let (ir, geometry) = composite_with_line_child();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    for cap in [1, 2] {
        let mut policy = DecodePolicy::service();
        // The composite enters first; its line child needs the second frame.
        policy.limits.max_recursion_depth = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut active = BTreeSet::new();
        let result = super::super::curve_geometry_coplanar(
            &geometry,
            &index,
            Transform::identity(),
            (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            0.001,
            &mut active,
            &ctx,
        );
        assert!(active.is_empty());
        if cap == 1 {
            let Err(CodecError::ResourceLimit(first)) = result else {
                panic!("expected child depth refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
            assert_eq!(first.operation, "iges coplanar curve recursion");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        } else {
            assert!(result.unwrap());
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn coplanar_removal_refusal_destroys_path_before_frame_storage() {
    let (ir, geometry) = composite_with_line_child();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges coplanar active removal",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut active = BTreeSet::new();
            let result = super::super::curve_geometry_coplanar(
                &geometry,
                &index,
                Transform::identity(),
                (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                0.001,
                &mut active,
                &ctx,
            );
            assert!(active.is_empty());
            let Err(CodecError::ResourceLimit(first)) = &result else {
                panic!("expected removal refusal")
            };
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first)
            );
            result
        },
    );
}

#[test]
fn transform_removal_refusal_destroys_path_before_frame_storage() {
    let entry = super::transform_entry(1, 0);
    let record = identity_record(1);
    let entries = BTreeMap::from([(1, &entry)]);
    let records = BTreeMap::from([(1, &record)]);
    let precision = crate::global::RealPrecision {
        single_significance: 6,
        double_significance: 15,
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges transform chain removal",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut path = BTreeSet::new();
            let result = super::super::resolve_transform(
                1, &entries, &records, 1.0, precision, &mut path, &ctx,
            )
            .map_err(|error| match error {
                super::super::TransformResolutionError::Resource(error) => error,
                other @ super::super::TransformResolutionError::Invalid(_) => {
                    panic!("unexpected invalid transform: {other:?}")
                }
            });
            assert!(path.is_empty());
            let Err(CodecError::ResourceLimit(first)) = &result else {
                panic!("expected removal refusal")
            };
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first)
            );
            result
        },
    );
}

fn node_bytes<T>() -> u64 {
    u64::try_from(
        11 * std::mem::size_of::<T>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<T>().max(std::mem::align_of::<usize>()),
    )
    .unwrap()
}

#[test]
fn transform_success_destroys_empty_root_with_exact_complete_work() {
    let directory: Vec<_> = [1, 3, 5, 7, 9, 11]
        .into_iter()
        .map(|sequence| {
            super::transform_entry(
                sequence,
                if sequence == 1 {
                    0
                } else {
                    i64::from(sequence - 2)
                },
            )
        })
        .collect();
    let parameters: Vec<_> = directory
        .iter()
        .map(|entry| identity_record(entry.sequence))
        .collect();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect();
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect();
    let node = node_bytes::<u32>();
    // Insertion pays ten node passes; six removals pay six more. Path key
    // work is 4*(3*(0+1+2+3+4+5)+(1+2+3+4+5+6)); the two maps cost 6*2*6*4.
    let work = 16 * node + 4 * (3 * 15 + 21) + 6 * 2 * 6 * 4;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2 * node;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 6;
    policy.limits.max_work_units = work;
    policy.limits.max_recursion_depth = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut path = BTreeSet::new();
    assert_eq!(
        super::super::resolve_transform(
            11,
            &entries,
            &records,
            1.0,
            crate::global::RealPrecision {
                single_significance: 6,
                double_significance: 15
            },
            &mut path,
            &ctx
        )
        .unwrap(),
        Transform::identity()
    );
    assert!(path.is_empty());
    let released = ctx
        .reserve_scoped(2 * node, "test destroyed transform root")
        .unwrap();
    drop(released);
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test complete transform work")
    else {
        panic!("expected exact transform work");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn coplanar_siblings_destroy_empty_roots_with_one_live_node_allowance() {
    let (ir, geometry) = composite_with_line_child();
    let SolvedCurveGeometry::Composite { segments, .. } = geometry else {
        unreachable!()
    };
    let segment = segments[0].clone();
    let geometry = SolvedCurveGeometry::Composite {
        segments: vec![segment.clone(), segment].try_into().unwrap(),
        self_intersect: Some(false),
    };
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let node = node_bytes::<CurveId>();
    let key = u64::try_from(ir.model.curves[0].id.as_str().len()).unwrap();
    for cap in [node + key - 1, node + key] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 2;
        policy.limits.max_recursion_depth = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut active = BTreeSet::new();
        let result = super::super::curve_geometry_coplanar(
            &geometry,
            &index,
            Transform::identity(),
            (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            0.001,
            &mut active,
            &ctx,
        );
        assert!(active.is_empty());
        if cap == node + key {
            assert!(result.unwrap());
            let released = ctx
                .reserve_scoped(cap, "test destroyed coplanar roots")
                .unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(first)) = result else {
                panic!("expected first active node refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges coplanar active curves");
            assert_eq!(
                (first.limit, first.used, first.additional),
                (cap, key, node)
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
    }
}

#[test]
fn coplanar_removal_preserves_seeded_ancestor_and_its_actual_storage() {
    let (ir, geometry) = composite_with_line_child();
    let index = ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    let seed = CurveId::mint("iges:model:curve#seed").unwrap();
    let seed_bytes = u64::try_from(seed.as_str().len()).unwrap();
    let child_bytes = u64::try_from(ir.model.curves[0].id.as_str().len()).unwrap();
    let node = node_bytes::<CurveId>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = node + seed_bytes + child_bytes;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx
        .reserve_scoped(0, "test seeded ancestor storage")
        .unwrap();
    let mut active = BTreeSet::new();
    storage
        .with_storage(|| {
            let key = seed.try_clone_for_decode(&ctx, "test seeded ancestor key")?;
            ctx.insert_btree_set(&mut active, key, "test seeded ancestor node")
        })
        .unwrap();
    assert!(super::super::curve_geometry_coplanar(
        &geometry,
        &index,
        Transform::identity(),
        (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        0.001,
        &mut active,
        &ctx
    )
    .unwrap());
    assert_eq!(active, BTreeSet::from([seed]));
    let child_released = ctx
        .reserve_scoped(child_bytes, "test removed child backing")
        .unwrap();
    drop(child_released);
    drop(active);
    drop(storage);
    let released = ctx
        .reserve_scoped(
            node + seed_bytes + child_bytes,
            "test destroyed seeded path",
        )
        .unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}
