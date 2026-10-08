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
        values.into_iter().map(|value| Token {
            value: TokenValue::real(value),
            span: 0..0,
        }).collect(),
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
    let error = super::super::resolve_transform(
        3, &entries, &records, 1.0, precision, &mut path, &ctx,
    ).unwrap_err();
    let super::super::TransformResolutionError::Resource(CodecError::ResourceLimit(first)) = error
    else { panic!("expected child depth refusal") };
    assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
    assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
    assert_eq!(first.operation, "iges_transform_chain");
    assert!(path.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));

    policy.limits.max_recursion_depth = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut path = BTreeSet::new();
    assert_eq!(super::super::resolve_transform(
        3, &entries, &records, 1.0, precision, &mut path, &ctx,
    ).unwrap(), cadmpeg_ir::transform::Transform::identity());
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
        }].try_into().unwrap(),
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
            &geometry, &index, Transform::identity(),
            (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            0.001, &mut active, &ctx,
        );
        assert!(active.is_empty());
        if cap == 1 {
            let Err(CodecError::ResourceLimit(first)) = result else {
                panic!("expected child depth refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
            assert_eq!(first.operation, "iges coplanar curve recursion");
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
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
        ResourceDimension::WorkUnits, "iges coplanar active removal", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut active = BTreeSet::new();
            let result = super::super::curve_geometry_coplanar(
                &geometry, &index, Transform::identity(),
                (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                0.001, &mut active, &ctx,
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

#[test]
fn transform_removal_refusal_destroys_path_before_frame_storage() {
    let entry = super::transform_entry(1, 0);
    let record = identity_record(1);
    let entries = BTreeMap::from([(1, &entry)]);
    let records = BTreeMap::from([(1, &record)]);
    let precision = crate::global::RealPrecision {
        single_significance: 6, double_significance: 15,
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "iges transform chain removal", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut path = BTreeSet::new();
            let result = super::super::resolve_transform(
                1, &entries, &records, 1.0, precision, &mut path, &ctx,
            ).map_err(|error| match error {
                super::super::TransformResolutionError::Resource(error) => error,
                other => panic!("unexpected invalid transform: {other:?}"),
            });
            assert!(path.is_empty());
            let Err(CodecError::ResourceLimit(first)) = &result else {
                panic!("expected removal refusal")
            };
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first));
            result
        },
    );
}
