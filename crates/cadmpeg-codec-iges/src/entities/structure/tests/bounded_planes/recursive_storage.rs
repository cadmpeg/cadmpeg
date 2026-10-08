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
